import { existsSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** Repo root (directory containing `Cargo.toml`). */
export function findRepoRoot(): string {
  const fromEnv = process.env.BEAVA_REPO_ROOT;
  if (fromEnv && existsSync(join(fromEnv, "Cargo.toml"))) {
    return fromEnv;
  }
  let dir = dirname(fileURLToPath(import.meta.url));
  for (let i = 0; i < 12; i++) {
    if (existsSync(join(dir, "Cargo.toml"))) {
      return dir;
    }
    const parent = dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  throw new Error(
    "Could not locate repo root (Cargo.toml). Set BEAVA_REPO_ROOT if needed.",
  );
}
