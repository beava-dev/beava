import type { ZodError } from "zod";

import type { BeavaWireErrorBody } from "./wire-schemas.js";

export type { BeavaWireErrorBody } from "./wire-schemas.js";

export class BeavaResponseValidationError extends Error {
  readonly zodError: ZodError;

  constructor(zodError: ZodError) {
    super(`Invalid beava success response: ${zodError.message}`);
    this.name = "BeavaResponseValidationError";
    this.zodError = zodError;
  }
}

export class BeavaError extends Error {
  readonly status: number;
  readonly code: string;
  readonly path: string;
  readonly errors: unknown[];

  constructor(
    status: number,
    wire: BeavaWireErrorBody,
    options?: ErrorOptions,
  ) {
    const msg =
      wire.message ??
      wire.reason ??
      `Beava request failed with HTTP ${String(status)}`;
    super(msg, options);
    this.name = "BeavaError";
    this.status = status;
    this.code = wire.code;
    this.path = wire.path ?? "";
    this.errors = wire.errors ?? [];
  }
}
