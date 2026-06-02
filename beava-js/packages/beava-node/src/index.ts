export {
  BeavaError,
  BeavaResponseValidationError,
  type BeavaWireErrorBody,
} from "./beava-error.js";
export {
  createBeavaClient,
  type BeavaClient,
  type BeavaClientOptions,
} from "./create-beava-client.js";
export {
  batchGetEntrySchema,
  batchGetRequestSchema,
  batchGetResponseSchema,
  beavaErrorEnvelopeSchema,
  beavaWireErrorBodySchema,
  entityKeySchema,
  getRequestSchema,
  jsonObjectSchema,
  jsonValueSchema,
  pingResponseSchema,
  pushRequestSchema,
  pushResponseSchema,
  registerRequestSchema,
  registerResponseSchema,
} from "./wire-schemas.js";
export type {
  BatchGetEntry,
  BatchGetRequest,
  EntityKey,
  GetRequest,
  JsonObject,
  JsonValue,
  PingResponse,
  PushRequest,
  PushResponse,
  RegisterRequest,
  RegisterResponse,
} from "./wire-schemas.js";
