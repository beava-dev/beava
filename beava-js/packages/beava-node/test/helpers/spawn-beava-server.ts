import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { findRepoRoot } from "./repo-root.js";

export type BeavaTestServer = {
  readonly httpUrl: string;
  readonly tcpUrl: string;
  close: () => Promise<void>;
};

function binaryPath(repoRoot: string): string {
  const name = process.platform === "win32" ? "beava.exe" : "beava";
  return join(repoRoot, "target", "debug", name);
}

/**
 * Spawns `target/debug/beava` with ephemeral ports (mirrors `python/tests/conftest.py::beava_server`).
 */
export async function spawnBeavaServer(): Promise<BeavaTestServer> {
  const repoRoot = findRepoRoot();
  const bin = binaryPath(repoRoot);
  const walDir = mkdtempSync(join(tmpdir(), "beava-js-wal-"));
  const snapDir = mkdtempSync(join(tmpdir(), "beava-js-snap-"));

  const proc: ChildProcessWithoutNullStreams = spawn(
    bin,
    ["--config", process.platform === "win32" ? "NUL" : "/dev/null"],
    {
      env: {
        ...process.env,
        BEAVA_LISTEN_ADDR: "127.0.0.1:0",
        BEAVA_ADMIN_ADDR: "127.0.0.1:0",
        BEAVA_TCP_PORT: "0",
        BEAVA_WAL_DIR: walDir,
        BEAVA_SNAPSHOT_DIR: snapDir,
        BEAVA_DEV_ENDPOINTS: "1",
      },
      stdio: ["ignore", "pipe", "ignore"],
    },
  );

  const httpAddrs: string[] = [];
  const tcpAddrs: string[] = [];

  await new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      proc.kill("SIGKILL");
      reject(
        new Error(
          `beava bind timeout (http=${httpAddrs.join()}, tcp=${tcpAddrs.join()})`,
        ),
      );
    }, 5_000);

    const rl = createInterface({ input: proc.stdout });

    proc.once("error", (err) => {
      clearTimeout(timeout);
      rl.close();
      reject(err);
    });

    proc.once("exit", (code, signal) => {
      if (httpAddrs.length > 0 && tcpAddrs.length > 0) return;
      clearTimeout(timeout);
      rl.close();
      reject(
        new Error(
          `beava exited before bind (code=${String(code)} signal=${String(signal)})`,
        ),
      );
    });

    rl.on("line", (line) => {
      try {
        const rec: unknown = JSON.parse(line);
        if (typeof rec !== "object" || rec === null) return;
        const o = rec as { kind?: string; addr?: string };
        if (o.kind === "server.http_bound" && o.addr) httpAddrs.push(o.addr);
        if (o.kind === "server.tcp_bound" && o.addr) tcpAddrs.push(o.addr);
        if (httpAddrs.length > 0 && tcpAddrs.length > 0) {
          clearTimeout(timeout);
          rl.close();
          resolve();
        }
      } catch {
        /* ignore non-JSON lines */
      }
    });
  });

  const httpUrl = `http://${httpAddrs[0]}`;
  const tcpUrl = `tcp://${tcpAddrs[0]}`;

  return {
    httpUrl,
    tcpUrl,
    close() {
      return new Promise<void>((resolve) => {
        if (proc.exitCode !== null || proc.signalCode !== null) {
          resolve();
          return;
        }
        const killTimer = setTimeout(() => {
          proc.kill("SIGKILL");
        }, 5_000);
        proc.once("exit", () => {
          clearTimeout(killTimer);
          resolve();
        });
        proc.kill("SIGTERM");
      });
    },
  };
}
