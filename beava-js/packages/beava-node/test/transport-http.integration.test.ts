import {
  describe,
  it,
  expect,
  beforeEach,
  afterEach,
} from "vitest";
import { createBeavaClient } from "../src/create-beava-client.js";
import { spawnBeavaServer } from "./helpers/spawn-beava-server.js";

const integration = process.env.BEAVA_INTEGRATION === "1";

/** Mirrors `python/tests/test_transport_http.py::VALID_REGISTER_PAYLOAD`. */
const validRegister = {
  nodes: [
    {
      kind: "event",
      name: "TestEvent",
      schema: {
        fields: { event_time: "i64", amount: "f64" },
        optional_fields: [],
      },
      dedupe_key: null,
      dedupe_window_ms: null,
      keep_events_for_ms: null,
    },
  ],
} as const;

const invalidRegister = {
  nodes: [
    {
      kind: "event",
      name: "_beava_reserved",
      schema: {
        fields: { x: "f64" },
        optional_fields: [],
      },
      dedupe_key: null,
      dedupe_window_ms: null,
      keep_events_for_ms: null,
    },
  ],
} as const;

describe.skipIf(!integration)("HTTP transport (integration, real beava)", () => {
  let server: Awaited<ReturnType<typeof spawnBeavaServer>>;

  beforeEach(async () => {
    server = await spawnBeavaServer();
  }, 120_000);

  afterEach(async () => {
    await server?.close();
  });

  it("register returns status ok and registry_version >= 1", async () => {
    const client = createBeavaClient({ baseUrl: server.httpUrl });
    const result = await client.register(validRegister);
    expect(result).toMatchObject({
      status: "ok",
    });
    expect(Number(result.registry_version)).toBeGreaterThanOrEqual(1);
  });

  it("invalid register raises BeavaError with invalid_registration", async () => {
    const client = createBeavaClient({ baseUrl: server.httpUrl });
    await expect(client.register(invalidRegister)).rejects.toMatchObject({
      name: "BeavaError",
      code: "invalid_registration",
    });
  });

  it("ping bumps registry_version after register", async () => {
    const client = createBeavaClient({ baseUrl: server.httpUrl });
    const pre = await client.ping();
    expect(pre.pong).toBe(true);
    const preV = pre.registry_version;
    await client.register({
      nodes: [
        {
          kind: "event",
          name: "PingBumpEvent",
          schema: {
            fields: { n: "i64" },
            optional_fields: [],
          },
          dedupe_key: null,
          dedupe_window_ms: null,
          keep_events_for_ms: null,
        },
      ],
    });
    const post = await client.ping();
    expect(post.pong).toBe(true);
    expect(post.registry_version).toBe(preV + 1);
  });
});
