"""Pipeline registered on beava.dev — the canonical SiteMetrics shape.

This file IS the example shown on https://beava.dev. To change what
beava.dev computes, edit this file (and the inline example in
`beava-website/project/index.html` to keep them in sync) and merge to
main; the deploy workflow regenerates the wire payload via this Python
SDK and POSTs it to the live `/register` endpoint **without `force`**.

Why `force=False` on auto-deploy:

  Server semantics for `force=true` are NOT "apply the new shape on top
  of existing state" — they're "pre-remove every descriptor whose diff
  classifies as destructive OR additive-against-existing, then re-add
  fresh." Pre-removal drops compiled chains, aggregations, feature
  index entries, and accumulated per-entity state. So `force=true` on
  every deploy silently wipes SiteMetrics whenever the SDK's
  `_to_register_json` output drifts (SDK version bump, dict ordering,
  default-field rendering) — drift classifies as
  additive-against-existing → pre-removes → clears state.

  With `force=False` the server returns 409 + `force_required` only if
  the diff is genuinely destructive; the deploy fails noisily and the
  operator opts in to the wipe by re-running with `--force` once.

  See `crates/beava-server/src/apply_shard.rs:264-348` for the full
  pre-removal block.

Usage:
  python register_pipeline.py --dump                    # additive payload (default)
  python register_pipeline.py --dump --force            # force=true payload
  python register_pipeline.py http://beava:8080         # register against server
  python register_pipeline.py http://beava:8080 --force
"""
from __future__ import annotations

import sys

import beava as bv


@bv.event
class PageView:
    session_id: str
    path: str
    dwell_ms: int  # set when the visitor leaves the page


@bv.table  # no key= → one row, site-wide (ADR-003)
def SiteMetrics(e: PageView):
    return e.agg(
        median_dwell_1h=bv.quantile("dwell_ms", q=0.5, window="1h"),
        page_views_today=bv.count(window="24h"),
        top_page_1h=bv.top_k("path", k=1, window="1h"),
    )


def _dump_payload(*, force: bool) -> bytes:
    """Render the wire-shape JSON the server expects."""
    # `_to_register_json` is the SDK's private renderer (App.register uses it
    # to build the POST body). Acceptable boundary-crossing for a deploy
    # script that needs the bytes without an actual transport.
    from beava._app import _to_register_json

    return _to_register_json((PageView, SiteMetrics), force=force)


def main() -> int:
    args = sys.argv[1:]
    force = "--force" in args
    args = [a for a in args if a != "--force"]
    if not args:
        print(
            "usage: register_pipeline.py {--dump | <server-url>} [--force]",
            file=sys.stderr,
        )
        return 2
    arg = args[0]
    if arg == "--dump":
        sys.stdout.buffer.write(_dump_payload(force=force))
        return 0
    with bv.App(arg) as app:
        app.register(PageView, SiteMetrics, force=force)
    print(f"OK: SiteMetrics + PageView registered at {arg} (force={force})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
