import { BeavaError, BeavaResponseValidationError } from "./beava-error.js";
import {
  batchGetRequestSchema,
  batchGetResponseSchema,
  beavaErrorEnvelopeSchema,
  getRequestSchema,
  jsonObjectSchema,
  pingResponseSchema,
  pushRequestSchema,
  pushResponseSchema,
  registerRequestSchema,
  registerResponseSchema,
} from "./wire-schemas.js";
import type {
  BatchGetEntry,
  BatchGetRequest,
  GetRequest,
  JsonObject,
  JsonValue,
  PingResponse,
  PushRequest,
  PushResponse,
  RegisterRequest,
  RegisterResponse,
} from "./wire-schemas.js";
import type { ZodType } from "zod";

const JSON_HEADERS = {
  "Content-Type": "application/json",
} as const;

export type BeavaClientOptions = {
  /** Base URL of the data plane, e.g. `http://127.0.0.1:8080` (no trailing path). */
  baseUrl: string;
  /** Per-request I/O timeout in seconds (default 30, matches Python `HttpTransport`). */
  timeoutSeconds?: number;
  /** Optional `fetch` override (tests or custom runtimes). */
  fetch?: typeof fetch;
  /** Extra headers merged on every request after defaults. */
  headers?: HeadersInit;
};

export type BeavaClient = {
  readonly ping: (signal?: AbortSignal) => Promise<PingResponse>;
  readonly register: (
    body: RegisterRequest,
    signal?: AbortSignal,
  ) => Promise<RegisterResponse>;
  readonly push: (
    body: PushRequest,
    signal?: AbortSignal,
  ) => Promise<PushResponse>;
  readonly get: (body: GetRequest, signal?: AbortSignal) => Promise<JsonObject>;
  readonly batchGet: (
    body: BatchGetRequest,
    signal?: AbortSignal,
  ) => Promise<JsonObject[]>;
  readonly reset: (signal?: AbortSignal) => Promise<void>;
};

function joinBasePath(baseUrl: string, path: string): string {
  const trimmed = baseUrl.replace(/\/+$/, "");
  const p = path.startsWith("/") ? path : `/${path}`;
  return `${trimmed}${p}`;
}

function combineAbortSignals(
  a: AbortSignal | undefined,
  b: AbortSignal | undefined,
): AbortSignal | undefined {
  if (!a && !b) return undefined;
  if (!a) return b;
  if (!b) return a;
  const c = new AbortController();
  const onAbort = (): void => {
    c.abort();
  };
  a.addEventListener("abort", onAbort, { once: true });
  b.addEventListener("abort", onAbort, { once: true });
  return c.signal;
}

function requestSignal(
  timeoutSeconds: number | undefined,
): AbortSignal | undefined {
  if (timeoutSeconds === undefined) return undefined;
  const ms = Math.max(1, Math.ceil(timeoutSeconds * 1000));
  return AbortSignal.timeout(ms);
}

function serialiseKey(key: GetRequest["key"]): JsonValue {
  return key as JsonValue;
}

function toWireBatchEntry(e: BatchGetEntry): Record<string, unknown> {
  if ("features" in e) {
    const row: Record<string, unknown> = {
      table: e.table,
      key: serialiseKey(e.key),
    };
    if (e.features !== null) {
      row.features = [...e.features];
    }
    return row;
  }
  return { table: e.table, key: serialiseKey(e.key) };
}

async function readJsonValue(res: Response): Promise<unknown> {
  const text = await res.text();
  if (text.length === 0) return {};
  try {
    return JSON.parse(text) as unknown;
  } catch {
    throw new BeavaError(res.status, {
      code: "unparseable_body",
      message: text.slice(0, 200),
    });
  }
}

function throwBeavaError(status: number, body: unknown): never {
  const envelope = beavaErrorEnvelopeSchema.safeParse(body);
  if (envelope.success) {
    throw new BeavaError(status, envelope.data.error);
  }
  throw new BeavaError(status, {
    code: "unknown",
    message: typeof body === "string" ? body : JSON.stringify(body),
  });
}

function parseSuccess<T>(schema: ZodType<T>, data: unknown): T {
  const r = schema.safeParse(data);
  if (!r.success) {
    throw new BeavaResponseValidationError(r.error);
  }
  return r.data;
}

async function postBeava(
  fetchImpl: typeof fetch,
  url: string,
  bodyBytes: string,
  signal: AbortSignal | undefined,
  extraHeaders: HeadersInit | undefined,
): Promise<unknown> {
  const headers = new Headers(JSON_HEADERS);
  if (extraHeaders) {
    new Headers(extraHeaders).forEach((v, k) => {
      headers.set(k, v);
    });
  }
  const res = await fetchImpl(url, {
    method: "POST",
    headers,
    body: bodyBytes,
    signal,
  });
  const parsed = await readJsonValue(res);
  if (res.status === 200) {
    return parsed;
  }
  throwBeavaError(res.status, parsed);
}

export function createBeavaClient(options: BeavaClientOptions): BeavaClient {
  const fetchImpl = options.fetch ?? globalThis.fetch;
  const timeoutSeconds = options.timeoutSeconds ?? 30;
  const extraHeaders = options.headers;
  const baseUrl = options.baseUrl;

  const run = async (
    path: string,
    bodyJson: string,
    perCallSignal?: AbortSignal,
  ): Promise<unknown> => {
    const url = joinBasePath(baseUrl, path);
    const signal = combineAbortSignals(
      perCallSignal,
      requestSignal(timeoutSeconds),
    );
    return postBeava(fetchImpl, url, bodyJson, signal, extraHeaders);
  };

  return {
    async ping(signal?: AbortSignal): Promise<PingResponse> {
      const out = await run("/ping", "{}", signal);
      return parseSuccess(pingResponseSchema, out);
    },

    async register(
      body: RegisterRequest,
      signal?: AbortSignal,
    ): Promise<RegisterResponse> {
      const parsed = registerRequestSchema.parse(body);
      const payload: Record<string, JsonValue> = {
        nodes: parsed.nodes.map((n) => ({ ...n })),
      };
      if (parsed.force === true) payload.force = true;
      if (parsed.dry_run === true) payload.dry_run = true;
      const out = await run("/register", JSON.stringify(payload), signal);
      return parseSuccess(registerResponseSchema, out);
    },

    async push(body: PushRequest, signal?: AbortSignal): Promise<PushResponse> {
      const parsed = pushRequestSchema.parse(body);
      const payload: Record<string, JsonValue> = {
        event: parsed.event,
        data: { ...parsed.data },
      };
      const out = await run("/push", JSON.stringify(payload), signal);
      return parseSuccess(pushResponseSchema, out);
    },

    async get(body: GetRequest, signal?: AbortSignal): Promise<JsonObject> {
      const parsed = getRequestSchema.parse(body);
      const payload: Record<string, JsonValue> = {
        table: parsed.table,
        key: serialiseKey(parsed.key),
      };
      if (parsed.features != null) {
        payload.features = [...parsed.features];
      }
      const out = await run("/get", JSON.stringify(payload), signal);
      return parseSuccess(jsonObjectSchema, out);
    },

    async batchGet(
      body: BatchGetRequest,
      signal?: AbortSignal,
    ): Promise<JsonObject[]> {
      const parsed = batchGetRequestSchema.parse(body);
      const wireRequests = parsed.requests.map((r) => toWireBatchEntry(r));
      const out = await run(
        "/batch_get",
        JSON.stringify({ requests: wireRequests }),
        signal,
      );
      const envelope = parseSuccess(batchGetResponseSchema, out);
      return envelope.results ?? [];
    },

    async reset(signal?: AbortSignal): Promise<void> {
      await run("/reset", "{}", signal);
    },
  };
}
