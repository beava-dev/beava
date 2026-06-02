import { z } from "zod";

export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export const jsonValueSchema: z.ZodType<JsonValue> = z.lazy(() =>
  z.union([
    z.null(),
    z.boolean(),
    z.number(),
    z.string(),
    z.array(jsonValueSchema),
    z.record(z.string(), jsonValueSchema),
  ]),
);

export const jsonObjectSchema = z.record(z.string(), jsonValueSchema);
export type JsonObject = z.infer<typeof jsonObjectSchema>;

export const entityKeySchema = z.union([
  z.string(),
  z.array(jsonValueSchema),
]);

export type EntityKey = z.infer<typeof entityKeySchema>;

export const registerRequestSchema = z
  .object({
    nodes: z.array(jsonObjectSchema),
    force: z.boolean().optional(),
    dry_run: z.boolean().optional(),
  })
  .strict();

export type RegisterRequest = z.infer<typeof registerRequestSchema>;

export const registerResponseSchema = jsonObjectSchema;
export type RegisterResponse = z.infer<typeof registerResponseSchema>;

export const pushRequestSchema = z
  .object({
    event: z.string().min(1),
    data: jsonObjectSchema,
  })
  .strict();

export type PushRequest = z.infer<typeof pushRequestSchema>;

export const pushResponseSchema = z
  .object({
    ack_lsn: z.number(),
    idempotent_replay: z.boolean(),
    registry_version: z.number(),
  })
  .passthrough();

export type PushResponse = z.infer<typeof pushResponseSchema>;

export const getRequestSchema = z
  .object({
    table: z.string().min(1),
    key: entityKeySchema,
    features: z.array(z.string()).nullable().optional(),
  })
  .strict();

export type GetRequest = z.infer<typeof getRequestSchema>;

export const batchGetEntrySchema = z.union([
  z
    .object({
      table: z.string().min(1),
      key: entityKeySchema,
    })
    .strict(),
  z
    .object({
      table: z.string().min(1),
      key: entityKeySchema,
      features: z.array(z.string()).nullable(),
    })
    .strict(),
]);

export type BatchGetEntry = z.infer<typeof batchGetEntrySchema>;

export const batchGetRequestSchema = z
  .object({
    requests: z.array(batchGetEntrySchema),
  })
  .strict();

export type BatchGetRequest = z.infer<typeof batchGetRequestSchema>;

export const batchGetResponseSchema = z
  .object({
    results: z.array(jsonObjectSchema).optional(),
  })
  .passthrough();

export const pingResponseSchema = z
  .object({
    pong: z.literal(true),
    registry_version: z.number(),
  })
  .strict();

export type PingResponse = z.infer<typeof pingResponseSchema>;

export const beavaWireErrorBodySchema = z
  .object({
    code: z.string(),
    path: z.string().optional(),
    message: z.string().optional(),
    reason: z.string().optional(),
    errors: z.array(z.unknown()).optional(),
  })
  .passthrough();

export type BeavaWireErrorBody = z.infer<typeof beavaWireErrorBodySchema>;

export const beavaErrorEnvelopeSchema = z
  .object({
    error: beavaWireErrorBodySchema,
  })
  .passthrough();
