// Ping IndexNow (Bing / Yandex / Seznam / Naver) with every URL in
// project/sitemap.xml. Run after a deploy when pages are added or
// titles/descriptions change.
//
// Run: node scripts/indexnow-submit.mjs
//
// The key file at project/<KEY>.txt must already be deployed to
// beava.dev — IndexNow verifies the request by fetching it.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PROJECT = path.resolve(__dirname, "..", "project");
const HOST = "beava.dev";
const KEY = "1e1a41d419ce23f9b703a37180718b26";
const KEY_LOC = `https://${HOST}/${KEY}.txt`;

const xml = fs.readFileSync(path.join(PROJECT, "sitemap.xml"), "utf-8");
const urlList = [...xml.matchAll(/<loc>([^<]+)<\/loc>/g)].map((m) => m[1]);
console.log(`Submitting ${urlList.length} URLs to IndexNow...`);

const body = JSON.stringify({ host: HOST, key: KEY, keyLocation: KEY_LOC, urlList });
const r = await fetch("https://api.indexnow.org/indexnow", {
  method: "POST",
  headers: { "Content-Type": "application/json; charset=utf-8" },
  body,
});
console.log(`IndexNow → ${r.status} ${r.statusText}`);
if (!r.ok) console.log(await r.text());
