# beava-js

[Turborepo](https://turbo.build/repo) workspace for the official Beava server-side TypeScript SDK.

`@beava/sdk` is the package users import from their server-side code. It exposes `createBeavaClient`, typed request and response shapes, Beava wire error classes, and Zod schemas for the HTTP data plane.

## Install

```sh
pnpm add @beava/sdk
```

Run a local server first:

```sh
beava
# default HTTP URL: http://127.0.0.1:8080
```

## Example

```ts
import { BeavaError, createBeavaClient } from "@beava/sdk";

const beava = createBeavaClient({
  baseUrl: "http://127.0.0.1:8080",
  timeoutSeconds: 10,
});

await beava.ping();

await beava.register({
  nodes: [
    {
      kind: "event",
      name: "Purchase",
      schema: {
        fields: {
          user_id: "str",
          amount: "f64",
        },
        optional_fields: [],
      },
      dedupe_key: null,
      dedupe_window_ms: null,
      keep_events_for_ms: null,
    },
  ],
});

await beava.push({
  event: "Purchase",
  data: {
    user_id: "alice",
    amount: 42.5,
  },
});
```

Read from a feature table that your deployed Beava pipeline has registered:

```ts
try {
  const row = await beava.get({ table: "UserSpend", key: "alice" });
  console.log(row);
} catch (error) {
  if (error instanceof BeavaError) {
    console.error(error.status, error.code, error.message);
  }
}
```

A runnable Node example lives at `examples/node-basic.mjs`. After `pnpm install` and `pnpm run build`, run it with:

```sh
BEAVA_URL=http://127.0.0.1:8080 node examples/node-basic.mjs
```

## API Surface

| Method                                | Wire route        | Notes                                  |
| ------------------------------------- | ----------------- | -------------------------------------- |
| `ping()`                              | `POST /ping`      | Liveness and registry version.         |
| `register({ nodes, force, dry_run })` | `POST /register`  | Registers event and table descriptors. |
| `push({ event, data })`               | `POST /push`      | Sends one event payload.               |
| `get({ table, key, features })`       | `POST /get`       | Reads one feature row.                 |
| `batchGet({ requests })`              | `POST /batch_get` | Reads many feature rows.               |
| `reset()`                             | `POST /reset`     | Test-mode only state reset.            |

Server error envelopes throw `BeavaError`. Malformed success responses throw `BeavaResponseValidationError`, which usually means the client and server versions disagree about the wire shape.

## Layout

| Path                      | Package         | Role                                                                                 |
| ------------------------- | --------------- | ------------------------------------------------------------------------------------ |
| `packages/beava-node`     | `@beava/sdk` | `createBeavaClient`, Zod wire schemas, Vitest unit + optional HTTP integration tests |
| `examples/node-basic.mjs` | example      | Connects to a running Beava server, registers an event, and pushes one row           |

Workspace members are defined in **`pnpm-workspace.yaml`** (`packages/beava-node`). The SDK extends **`tsconfig.node-library.json`** at this directory root (no separate TypeScript config package).

## Prerequisites

- **Node** `>=18` (see root **`package.json`** `engines`)
- **pnpm** `9.x` via [Corepack](https://nodejs.org/api/corepack.html): `corepack enable pnpm`

## Commands

From **`beava-js/`**:

```sh
pnpm install
pnpm run build          # tsc emit for publishable packages
pnpm run lint           # eslint across workspace
pnpm run check-types    # tsc --noEmit
pnpm run test           # Vitest (see below)
```

Scoped examples:

```sh
pnpm exec turbo run build test --filter=@beava/sdk
```

## Tests

**`@beava/sdk`** uses [Vitest](https://vitest.dev/). Default **`pnpm run test`** runs **unit tests** only (mocked `fetch`).

**HTTP integration tests** (real `beava` subprocess, same idea as `python/tests/test_transport_http.py`) run when:

1. The **`beava`** binary exists at **`target/debug/beava`** (repo root: run **`cargo build --bin beava`** from the Beava repo root), and
2. You set **`BEAVA_INTEGRATION=1`**. Optionally set **`BEAVA_REPO_ROOT`** to the Beava git root if discovery fails.

```sh
# from beava-js/
BEAVA_INTEGRATION=1 BEAVA_REPO_ROOT=/path/to/beava pnpm exec turbo run test --filter=@beava/sdk
```

CI sets these when running the **`beava-js`** job in **`.github/workflows/ci.yml`**.

## Repo checks

From the **Beava repo root**, **`bash .github/scripts/check.sh`** can run Rust, Python, and this tree together. **`bash .github/scripts/check.sh --js`** runs **`pnpm install`** and **`turbo run lint check-types test`** under **`beava-js/`** only. If **`target/debug/beava`** exists (from **`cargo build --bin beava`**), **`BEAVA_INTEGRATION=1`** is set so all Vitest tests run; otherwise three HTTP integration tests are skipped and the log notes why.

## Registry publish

1. Bump **`version`** in **`packages/beava-node/package.json`**.
2. From **`beava-js/`**: **`pnpm install`**, **`pnpm run build`**, **`pnpm run test`** (with integration if you use **`BEAVA_INTEGRATION=1`**).
3. **`pnpm publish --filter @beava/sdk --access public`** (uses **`prepack`** to run **`tsc`**; add **`--dry-run`** or **`pnpm pack --filter @beava/sdk`** to inspect the tarball).

Use **`pnpm config set //registry.npmjs.org/:_authToken`** if you publish with an auth token. Scoped **`@beava/*`** packages need **`publishConfig.access`** (already **`public`**).

## Links

- [Turborepo tasks and filters](https://turbo.build/repo/docs/crafting-your-repository/running-tasks)
- Beava repo: [github.com/beava-dev/beava](https://github.com/beava-dev/beava)
