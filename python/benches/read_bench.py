"""``python/benches/read_bench.py`` — Phase 19.4 Python read-bench harness.

Drives ``POST /get`` against a beava server warmed up with N synthetic events.
Captures req/sec, key-features/sec, and p50/p95/p99 latency for /get batch reads.

Single-file harness — spawns the beava server as a subprocess, registers the
fraud-team pipeline, warms up state with --warmup-events events using the same
zipfian shape that --total-reads will read from, then runs N async batch /get
requests via httpx.AsyncClient at concurrency --parallel.

CLI mirrors blast.py where applicable. NOT multi-process: read traffic is
trivially parallelisable inside one process via asyncio.

Output (stderr lines, parseable):
  beava-readbench: requests=N errors=N wall_clock_ms=N requests_per_sec=R \\
      key_features_per_sec=KF
  beava-readbench: latency_p50_us=N p95_us=N p99_us=N

The server lifecycle is owned by this script: it spawns ``target/release/beava``
with a generated YAML config (the binary takes ``--config <path>``; there are
no individual ``--http-port`` / ``--tcp-port`` flags), polls /health to
confirm bind, and SIGTERMs on exit (try/finally guarantees cleanup).

Cross-reference with the Phase 19.4 push bench (`blast.py`) — that harness
measures /push/{event}; this one measures /get.
"""

from __future__ import annotations

import argparse
import asyncio
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

# Make ``import beava`` and ``from benches...`` work either as a script or as
# a module. Mirrors blast.py.
_BENCH_DIR = Path(__file__).resolve().parent
_PYTHON_DIR = _BENCH_DIR.parent
if str(_PYTHON_DIR) not in sys.path:
    sys.path.insert(0, str(_PYTHON_DIR))

import httpx  # noqa: E402

from benches._configs import (  # noqa: E402
    extra_fields as cfg_extra_fields,
)
from benches._configs import (  # noqa: E402
    key_field as cfg_key_field,
)
from benches._configs import (  # noqa: E402
    load_pipeline_config,
    register_payload,
)
from benches.blast_shape import PoolConfig, build_pool  # noqa: E402

_REPO_ROOT = _PYTHON_DIR.parent
_BEAVA_BIN = _REPO_ROOT / "target" / "release" / "beava"


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    """Parse CLI args."""
    p = argparse.ArgumentParser(
        prog="python read_bench.py",
        description=(
            "Beava Python read-bench harness — Phase 19.4. "
            "Spawns beava server, warms up state, drives POST /get via "
            "httpx.AsyncClient, reports req/sec + p50/p95/p99 latency."
        ),
    )
    p.add_argument(
        "--pipeline",
        default="fraud-team",
        help=(
            "Pipeline config (resolved to crates/beava-bench/configs/<name>.json) "
            "or full path. Defaults to fraud-team."
        ),
    )
    p.add_argument(
        "--total-reads",
        type=int,
        default=50_000,
        help="Number of /get batch requests to issue (default 50_000).",
    )
    p.add_argument(
        "--warmup-events",
        type=int,
        default=100_000,
        help="Number of /push events to send before reads start (default 100_000).",
    )
    p.add_argument(
        "--keys-per-request",
        type=int,
        default=100,
        help="Keys per /get batch (default 100).",
    )
    p.add_argument(
        "--features-per-request",
        type=int,
        default=5,
        help="Features per /get batch (default 5).",
    )
    p.add_argument(
        "--parallel",
        type=int,
        default=8,
        help="asyncio concurrency for /get requests (default 8).",
    )
    p.add_argument(
        "--server-port",
        type=int,
        default=18080,
        help="HTTP port for spawned beava server (default 18080).",
    )
    p.add_argument(
        "--tcp-port",
        type=int,
        default=18081,
        help="TCP port for spawned beava server (default 18081).",
    )
    p.add_argument(
        "--zipf-alpha",
        type=float,
        default=1.0,
        help="Zipfian alpha for warmup + read keys (default 1.0).",
    )
    p.add_argument(
        "--cardinality",
        type=int,
        default=10_000,
        help="Distinct keys (default 10_000 — warmup hits each ~10x at 100k events).",
    )
    p.add_argument(
        "--seed",
        type=int,
        default=0xCAFEBABE,
        help="RNG seed for reproducible pool builds.",
    )
    p.add_argument(
        "--io-threads",
        type=int,
        default=None,
        help="Optional BEAVA_IO_THREADS override for spawned server (default: inherit env).",
    )
    return p.parse_args(argv)


# ─── Server lifecycle ─────────────────────────────────────────────────────────


def _write_server_config(http_port: int, tcp_port: int, wal_dir: Path, snapshot_dir: Path) -> Path:
    """Write a YAML config the spawned beava server can load via --config."""
    cfg_text = f"""listen_addr: "127.0.0.1:{http_port}"
log_level: warn
tcp:
  enabled: true
  host: "127.0.0.1"
  port: {tcp_port}
durability:
  wal_dir: "{wal_dir}"
  snapshot_dir: "{snapshot_dir}"
"""
    cfg_path = wal_dir.parent / "beava-readbench.yaml"
    cfg_path.write_text(cfg_text)
    return cfg_path


def _spawn_server(
    cfg_path: Path,
    http_port: int,
    io_threads: int | None,
) -> subprocess.Popen[bytes]:
    """Spawn beava and wait for /health to bind. Aborts after 10s."""
    if not _BEAVA_BIN.is_file():
        raise SystemExit(f"ERROR: beava binary not found at {_BEAVA_BIN}")
    env = os.environ.copy()
    if io_threads is not None:
        env["BEAVA_IO_THREADS"] = str(io_threads)
    proc = subprocess.Popen(
        [str(_BEAVA_BIN), "--config", str(cfg_path)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        env=env,
    )
    # Poll /health
    deadline = time.time() + 10.0
    last_err: Exception | None = None
    while time.time() < deadline:
        if proc.poll() is not None:
            stderr = (proc.stderr.read() if proc.stderr else b"").decode(errors="replace")
            raise SystemExit(
                f"ERROR: beava exited during startup (rc={proc.returncode})\nstderr:\n{stderr}"
            )
        try:
            r = httpx.get(f"http://127.0.0.1:{http_port}/health", timeout=0.5)
            if r.status_code == 200:
                return proc
        except Exception as e:  # noqa: BLE001
            last_err = e
        time.sleep(0.1)
    proc.terminate()
    raise SystemExit(
        f"ERROR: beava /health did not respond within 10s (last_err={last_err!r})"
    )


def _kill_server(proc: subprocess.Popen[bytes]) -> None:
    """Best-effort cleanup."""
    if proc.poll() is None:
        try:
            proc.terminate()
            try:
                proc.wait(timeout=5.0)
            except subprocess.TimeoutExpired:
                proc.send_signal(signal.SIGKILL)
                proc.wait(timeout=2.0)
        except Exception:  # noqa: BLE001
            pass


# ─── Warmup ────────────────────────────────────────────────────────────────────


def _warmup(http_url: str, cfg: dict[str, Any], n_events: int, seed: int,
            zipf_alpha: float, cardinality: int) -> set[str]:
    """Push N events via /push/{event}. Returns the set of key strings actually pushed
    (so reads target known-warm keys rather than gambling on the zipfian distribution).

    Uses the same blast_shape pool builder as blast.py — keys come from a zipfian
    distribution, so the most-frequent keys land in state more times. Reads sample
    from the SAME distribution so they hit hot keys with high probability.
    """
    extras = cfg_extra_fields(cfg)
    pipeline_event_name = str(cfg["event_name"])
    pipeline_key_field = cfg_key_field(cfg)

    pool_cfg = PoolConfig(
        shape="zipfian",
        wire_format="json",
        transport="http",
        cardinality=cardinality,
        zipf_alpha=zipf_alpha,
        mixed_event_count=1,
        seed=seed,
        pipeline_event_name=pipeline_event_name,
        pipeline_key_field=pipeline_key_field,
        pipeline_extra_fields=extras,
        mixed_event_names=[pipeline_event_name],
    )
    pool = build_pool(pool_cfg, n_events)

    pushed_keys: set[str] = set()
    with httpx.Client(base_url=http_url, timeout=30.0) as c:
        for item in pool:
            body = item.body
            try:
                key_val = body.get(pipeline_key_field)
                if key_val is not None:
                    pushed_keys.add(str(key_val))
            except AttributeError:
                pass
            payload = json.dumps(body, ensure_ascii=False).encode("utf-8")
            r = c.post(
                f"/push/{item.event_name}",
                content=payload,
                headers={"Content-Type": "application/json"},
            )
            if r.status_code != 200:
                raise SystemExit(
                    f"warmup push failed: status={r.status_code} body={r.text!r}"
                )
    return pushed_keys


# ─── Read driver ───────────────────────────────────────────────────────────────


async def _read_driver(
    http_url: str,
    keys: list[str],
    features: list[str],
    total_reads: int,
    keys_per_request: int,
    parallel: int,
    seed: int,
) -> tuple[int, int, list[float], float]:
    """Run total_reads /get batches at concurrency=parallel.

    Each request samples ``keys_per_request`` keys from the warmed-up key set.
    Sampling is uniform across the warm keyset (a stronger guarantee than zipfian-
    sampling that may miss some keys in short bursts). The point of this bench
    is to measure /get throughput, not to overlay a zipfian read shape on a
    zipfian write shape.

    Returns (ok_count, err_count, per_request_latencies_us, wall_clock_s).
    """
    import random as _random

    rng = _random.Random(seed ^ 0xFEEDFACE)
    if not keys:
        raise SystemExit("ERROR: warmup produced no keys; cannot run reads")

    # Pre-build all request bodies up front so the driver loop spends time on
    # the wire, not on Python random.choices in the hot path.
    bodies: list[bytes] = []
    for _ in range(total_reads):
        # Sample with replacement — ok if a request has dupes, /get tolerates it.
        sampled = [rng.choice(keys) for _ in range(keys_per_request)]
        body = {"keys": sampled, "features": features}
        bodies.append(json.dumps(body).encode("utf-8"))

    sem = asyncio.Semaphore(parallel)
    latencies_us: list[float] = []
    ok = 0
    err = 0
    err_lock = asyncio.Lock()

    limits = httpx.Limits(max_connections=parallel * 2, max_keepalive_connections=parallel * 2)
    async with httpx.AsyncClient(
        base_url=http_url,
        timeout=30.0,
        limits=limits,
        http2=False,
    ) as client:
        async def _one(body: bytes) -> None:
            nonlocal ok, err
            async with sem:
                t0 = time.perf_counter()
                try:
                    r = await client.post(
                        "/get",
                        content=body,
                        headers={"Content-Type": "application/json"},
                    )
                    elapsed_us = (time.perf_counter() - t0) * 1_000_000
                    if r.status_code == 200:
                        ok += 1
                        latencies_us.append(elapsed_us)
                    else:
                        async with err_lock:
                            err += 1
                            if err <= 3:
                                print(
                                    f"  /get error: status={r.status_code} body={r.text[:200]!r}",
                                    file=sys.stderr,
                                )
                except Exception as e:  # noqa: BLE001
                    async with err_lock:
                        err += 1
                        if err <= 3:
                            print(f"  /get exception: {e!r}", file=sys.stderr)

        wall_t0 = time.perf_counter()
        tasks = [asyncio.create_task(_one(b)) for b in bodies]
        await asyncio.gather(*tasks)
        wall_elapsed = time.perf_counter() - wall_t0

    return ok, err, latencies_us, wall_elapsed


# ─── Main ──────────────────────────────────────────────────────────────────────


def main(argv: list[str] | None = None) -> int:
    """Entry point."""
    args = parse_args(argv)
    cfg = load_pipeline_config(args.pipeline)

    pipeline_features: list[str] = list(cfg.get("features", []))
    if len(pipeline_features) < args.features_per_request:
        print(
            f"WARNING: pipeline has only {len(pipeline_features)} features in 'features' list; "
            f"requested {args.features_per_request}. Using all available.",
            file=sys.stderr,
        )
    chosen_features = pipeline_features[: args.features_per_request]
    print(f"  features: {chosen_features}", file=sys.stderr)

    # Per-run scratch dir for WAL/snapshot/config
    scratch = Path(tempfile.mkdtemp(prefix="beava-readbench-"))
    wal_dir = scratch / "wal"
    snapshot_dir = scratch / "snap"
    wal_dir.mkdir()
    snapshot_dir.mkdir()
    cfg_path = _write_server_config(
        http_port=args.server_port,
        tcp_port=args.tcp_port,
        wal_dir=wal_dir,
        snapshot_dir=snapshot_dir,
    )

    server_proc: subprocess.Popen[bytes] | None = None
    rc = 0
    try:
        print(f"  spawning beava: {_BEAVA_BIN} --config {cfg_path}", file=sys.stderr)
        server_proc = _spawn_server(cfg_path, args.server_port, args.io_threads)
        http_url = f"http://127.0.0.1:{args.server_port}"

        # Register pipeline.
        with httpx.Client(base_url=http_url, timeout=30.0) as c:
            r = c.post(
                "/register",
                content=register_payload(cfg),
                headers={"Content-Type": "application/json"},
            )
            if r.status_code != 200:
                raise SystemExit(
                    f"register failed: status={r.status_code} body={r.text!r}"
                )
        print("  /register OK", file=sys.stderr)

        # Warmup phase.
        warmup_t0 = time.perf_counter()
        pushed_keys = _warmup(
            http_url=http_url,
            cfg=cfg,
            n_events=args.warmup_events,
            seed=args.seed,
            zipf_alpha=args.zipf_alpha,
            cardinality=args.cardinality,
        )
        warmup_elapsed = time.perf_counter() - warmup_t0
        print(
            f"  warmup: {args.warmup_events} events pushed in {warmup_elapsed:.1f}s "
            f"({len(pushed_keys)} distinct keys)",
            file=sys.stderr,
        )

        # Run reads.
        keys_list = sorted(pushed_keys)
        ok, err, latencies_us, wall_elapsed = asyncio.run(
            _read_driver(
                http_url=http_url,
                keys=keys_list,
                features=chosen_features,
                total_reads=args.total_reads,
                keys_per_request=args.keys_per_request,
                parallel=args.parallel,
                seed=args.seed,
            )
        )

        wall_clock_ms = int(round(wall_elapsed * 1000))
        req_per_sec = ok / max(wall_elapsed, 1e-9)
        kf_per_sec = (ok * args.keys_per_request * len(chosen_features)) / max(wall_elapsed, 1e-9)

        if latencies_us:
            sorted_lat = sorted(latencies_us)
            n = len(sorted_lat)
            p50 = sorted_lat[int(n * 0.50)]
            p95 = sorted_lat[min(int(n * 0.95), n - 1)]
            p99 = sorted_lat[min(int(n * 0.99), n - 1)]
        else:
            p50 = p95 = p99 = 0.0

        print(
            f"beava-readbench: requests={ok} errors={err} "
            f"wall_clock_ms={wall_clock_ms} "
            f"requests_per_sec={req_per_sec:.0f} "
            f"key_features_per_sec={kf_per_sec:.0f}",
            file=sys.stderr,
        )
        print(
            f"beava-readbench: latency_p50_us={p50:.0f} "
            f"p95_us={p95:.0f} p99_us={p99:.0f}",
            file=sys.stderr,
        )
        # Stdout line — easy to grep-and-parse for orchestration scripts:
        print(
            f"READBENCH_CSV,{args.pipeline},{ok},{err},{wall_clock_ms},"
            f"{req_per_sec:.0f},{kf_per_sec:.0f},{p50:.0f},{p95:.0f},{p99:.0f}"
        )

        if err > 0 and ok == 0:
            rc = 3

    finally:
        if server_proc is not None:
            _kill_server(server_proc)
        try:
            shutil.rmtree(scratch, ignore_errors=True)
        except Exception:  # noqa: BLE001
            pass

    return rc


if __name__ == "__main__":
    raise SystemExit(main())
