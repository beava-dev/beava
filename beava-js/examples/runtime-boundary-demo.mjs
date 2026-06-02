import { createServer } from "node:http";

import { BeavaError, createBeavaClient } from "../packages/beava-node/dist/index.js";

const beavaUrl = process.env.BEAVA_URL ?? "http://127.0.0.1:18080";
const port = Number(process.env.PORT ?? "18100");
const adminToken = process.env.DEMO_ADMIN_TOKEN ?? "local-demo-secret";

const beava = createBeavaClient({
  baseUrl: beavaUrl,
  timeoutSeconds: 10,
});

const page = String.raw`<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Beava Runtime Boundary Demo</title>
    <style>
      body {
        color: #172033;
        font-family:
          ui-sans-serif,
          system-ui,
          -apple-system,
          BlinkMacSystemFont,
          "Segoe UI",
          sans-serif;
        line-height: 1.5;
        margin: 2rem auto;
        max-width: 920px;
        padding: 0 1rem;
      }
      button {
        cursor: pointer;
        margin: 0.25rem 0.5rem 0.25rem 0;
        padding: 0.55rem 0.8rem;
      }
      code,
      pre {
        background: #f5f7fb;
        border-radius: 6px;
      }
      code {
        padding: 0.1rem 0.25rem;
      }
      pre {
        min-height: 9rem;
        overflow: auto;
        padding: 1rem;
      }
      .grid {
        display: grid;
        gap: 1rem;
        grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
      }
      .card {
        border: 1px solid #d8deea;
        border-radius: 10px;
        padding: 1rem;
      }
    </style>
  </head>
  <body>
    <h1>Beava Runtime Boundary Demo</h1>
    <p>
      The Node server has already run the trusted setup path:
      <code>reset</code> and <code>register</code>. This browser page only gets
      public routes for <code>ping</code> and <code>push</code>.
    </p>
    <div class="grid">
      <section class="card">
        <h2>Browser/client-safe path</h2>
        <button id="ping">Ping through public API</button>
        <button id="push">Push PageView event</button>
      </section>
      <section class="card">
        <h2>Blocked browser control path</h2>
        <button id="reset">Try reset without server token</button>
        <p>
          This should fail with <code>403</code>. The browser does not receive
          the admin token.
        </p>
      </section>
    </div>
    <h2>Output</h2>
    <pre id="out">Ready.</pre>
    <script>
      const out = document.querySelector("#out");
      const write = (label, value) => {
        out.textContent =
          label + "\n" + JSON.stringify(value, null, 2) + "\n\n" + out.textContent;
      };
      const post = async (path, body = {}) => {
        const res = await fetch(path, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(body),
        });
        const text = await res.text();
        const data = text ? JSON.parse(text) : {};
        if (!res.ok) {
          return { ok: false, status: res.status, data };
        }
        return { ok: true, status: res.status, data };
      };

      document.querySelector("#ping").addEventListener("click", async () => {
        write("public ping", await post("/api/public/ping"));
      });

      document.querySelector("#push").addEventListener("click", async () => {
        write(
          "public push",
          await post("/api/public/page-view", {
            user_id: "browser-user",
            path: window.location.pathname,
          }),
        );
      });

      document.querySelector("#reset").addEventListener("click", async () => {
        write("blocked reset", await post("/api/admin/reset"));
      });
    </script>
  </body>
</html>`;

const pageViewDescriptor = {
  nodes: [
    {
      kind: "event",
      name: "PageView",
      schema: {
        fields: {
          user_id: "str",
          path: "str",
        },
        optional_fields: [],
      },
      dedupe_key: null,
      dedupe_window_ms: null,
      keep_events_for_ms: null,
    },
  ],
};

function json(res, status, body) {
  res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify(body));
}

function readJson(req) {
  return new Promise((resolve, reject) => {
    let body = "";
    req.setEncoding("utf8");
    req.on("data", (chunk) => {
      body += chunk;
    });
    req.on("end", () => {
      try {
        resolve(body ? JSON.parse(body) : {});
      } catch (error) {
        reject(error);
      }
    });
    req.on("error", reject);
  });
}

function toError(error) {
  if (error instanceof BeavaError) {
    return {
      status: error.status,
      code: error.code,
      message: error.message,
      path: error.path,
      errors: error.errors,
    };
  }
  return { message: error instanceof Error ? error.message : String(error) };
}

async function setupTrustedServerState() {
  await beava.reset();
  await beava.register(pageViewDescriptor);
}

const server = createServer(async (req, res) => {
  try {
    if (req.method === "GET" && req.url === "/") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      res.end(page);
      return;
    }

    if (req.method === "POST" && req.url === "/api/public/ping") {
      json(res, 200, await beava.ping());
      return;
    }

    if (req.method === "POST" && req.url === "/api/public/page-view") {
      const body = await readJson(req);
      json(
        res,
        200,
        await beava.push({
          event: "PageView",
          data: {
            user_id: String(body.user_id ?? "anonymous"),
            path: String(body.path ?? "/"),
          },
        }),
      );
      return;
    }

    if (req.method === "POST" && req.url === "/api/admin/reset") {
      if (req.headers["x-demo-admin-token"] !== adminToken) {
        json(res, 403, {
          error: "server_token_required",
          message: "Browser/client code does not receive the demo admin token.",
        });
        return;
      }
      await setupTrustedServerState();
      json(res, 200, { status: "reset_and_registered" });
      return;
    }

    json(res, 404, { error: "not_found" });
  } catch (error) {
    json(res, error instanceof BeavaError ? error.status : 500, {
      error: toError(error),
    });
  }
});

await setupTrustedServerState();

server.listen(port, "127.0.0.1", () => {
  console.log(`runtime boundary demo: http://127.0.0.1:${String(port)}`);
  console.log(`beava server: ${beavaUrl}`);
  console.log(`trusted admin token: ${adminToken}`);
});
