import {
  BeavaError,
  createBeavaClient,
} from "../packages/beava-node/dist/index.js";

const baseUrl = process.env.BEAVA_URL ?? "http://127.0.0.1:8080";

const beava = createBeavaClient({
  baseUrl,
  timeoutSeconds: 10,
});

try {
  const ping = await beava.ping();
  console.log("connected", ping);

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

  const ack = await beava.push({
    event: "Purchase",
    data: {
      user_id: "alice",
      amount: 42.5,
    },
  });

  console.log("pushed", ack);
} catch (error) {
  if (error instanceof BeavaError) {
    console.error("beava error", {
      status: error.status,
      code: error.code,
      message: error.message,
      path: error.path,
      errors: error.errors,
    });
  } else {
    console.error(error);
  }
  process.exitCode = 1;
}
