import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  BeavaError,
  BeavaResponseValidationError,
} from "../src/beava-error.js";
import { createBeavaClient } from "../src/create-beava-client.js";
import { registerRequestSchema } from "../src/wire-schemas.js";

function jsonResponse(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

describe("createBeavaClient (unit, mocked fetch)", () => {
  let fetchMock: ReturnType<typeof vi.fn>;

  beforeEach(() => {
    fetchMock = vi.fn();
  });

  it("POST /ping with Content-Type and empty JSON body", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({ pong: true, registry_version: 7 }),
    );
    const client = createBeavaClient({
      baseUrl: "http://127.0.0.1:9999/",
      fetch: fetchMock as typeof fetch,
    });
    const out = await client.ping();
    expect(out).toEqual({ pong: true, registry_version: 7 });
    expect(fetchMock).toHaveBeenCalledTimes(1);
    const [url, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(url).toBe("http://127.0.0.1:9999/ping");
    expect(init.method).toBe("POST");
    expect((init.headers as Headers).get("Content-Type")).toBe(
      "application/json",
    );
    expect(init.body).toBe("{}");
  });

  it("merges custom headers", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({ pong: true, registry_version: 1 }),
    );
    const client = createBeavaClient({
      baseUrl: "http://h.test",
      fetch: fetchMock as typeof fetch,
      headers: { "X-Test": "1" },
    });
    await client.ping();
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    const h = new Headers(init.headers as HeadersInit);
    expect(h.get("Content-Type")).toBe("application/json");
    expect(h.get("X-Test")).toBe("1");
  });

  it("push sends { event, data }", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse({
        ack_lsn: 1,
        idempotent_replay: false,
        registry_version: 2,
      }),
    );
    const client = createBeavaClient({
      baseUrl: "http://h.test",
      fetch: fetchMock as typeof fetch,
    });
    await client.push({ event: "Ev", data: { x: 1 } });
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(JSON.parse(init.body as string)).toEqual({
      event: "Ev",
      data: { x: 1 },
    });
  });

  it("get omits features when null", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ visits: 2 }));
    const client = createBeavaClient({
      baseUrl: "http://h.test",
      fetch: fetchMock as typeof fetch,
    });
    await client.get({
      table: "t",
      key: "alice",
      features: null,
    });
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(JSON.parse(init.body as string)).toEqual({
      table: "t",
      key: "alice",
    });
  });

  it("batch_get serialises tuple-style entries", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ results: [{ a: 1 }] }));
    const client = createBeavaClient({
      baseUrl: "http://h.test",
      fetch: fetchMock as typeof fetch,
    });
    await client.batchGet({
      requests: [
        { table: "t1", key: "k1" },
        { table: "t2", key: "k2", features: null },
        { table: "t3", key: "k3", features: ["f"] },
      ],
    });
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(JSON.parse(init.body as string)).toEqual({
      requests: [
        { table: "t1", key: "k1" },
        { table: "t2", key: "k2" },
        { table: "t3", key: "k3", features: ["f"] },
      ],
    });
  });

  it("reset sends {}", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ reset: true }));
    const client = createBeavaClient({
      baseUrl: "http://h.test",
      fetch: fetchMock as typeof fetch,
    });
    await client.reset();
    const [, init] = fetchMock.mock.calls[0] as [string, RequestInit];
    expect(init.body).toBe("{}");
  });

  it("maps 4xx JSON error envelope to BeavaError", async () => {
    fetchMock.mockResolvedValueOnce(
      jsonResponse(
        {
          error: {
            code: "invalid_registration",
            path: "/register",
            message: "bad",
          },
        },
        400,
      ),
    );
    const client = createBeavaClient({
      baseUrl: "http://h.test",
      fetch: fetchMock as typeof fetch,
    });
    await expect(
      client.register({ nodes: [{ kind: "event", name: "x" }] }),
    ).rejects.toMatchObject({
      name: "BeavaError",
      code: "invalid_registration",
      status: 400,
    });
  });

  it("throws BeavaResponseValidationError on malformed success ping body", async () => {
    fetchMock.mockResolvedValueOnce(jsonResponse({ pong: false }));
    const client = createBeavaClient({
      baseUrl: "http://h.test",
      fetch: fetchMock as typeof fetch,
    });
    await expect(client.ping()).rejects.toBeInstanceOf(
      BeavaResponseValidationError,
    );
  });

  it("throws BeavaError on invalid JSON error body", async () => {
    fetchMock.mockResolvedValueOnce(
      new Response("not-json", { status: 500, statusText: "ERR" }),
    );
    const client = createBeavaClient({
      baseUrl: "http://h.test",
      fetch: fetchMock as typeof fetch,
    });
    await expect(client.ping()).rejects.toMatchObject({
      name: "BeavaError",
      code: "unparseable_body",
    });
  });
});

describe("registerRequestSchema", () => {
  it("rejects unknown keys (strict)", () => {
    const r = registerRequestSchema.safeParse({
      nodes: [],
      extra: 1,
    });
    expect(r.success).toBe(false);
  });
});

describe("BeavaError", () => {
  it("prefers message over reason in constructor", () => {
    const e = new BeavaError(400, {
      code: "x",
      message: "m",
      reason: "r",
    });
    expect(e.message).toContain("m");
  });
});
