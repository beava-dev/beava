// Build SEO artifacts for the static site:
//   1. Inject Open Graph + Twitter Card + canonical-URL meta tags into
//      every project/**/index.html, derived from each page's existing
//      <title> and <meta name="description">.
//   2. Emit project/sitemap.xml listing every indexable URL.
//   3. Emit project/robots.txt pointing at the sitemap.
//
// Idempotent: pages already containing an `<meta property="og:title"` tag
// are skipped on the injection pass (their tags are kept as-is). The
// sitemap and robots.txt are always regenerated.
//
// Skips:
//   - design-system/  (internal; not user-facing)
//   - _pagefind/      (search index)
//   - 404.html        (no canonical; noindex anyway)
//
// Run with: node scripts/build-seo.mjs
// Or: npm run build:seo

import fs from "node:fs";
import path from "node:path";
import { execSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const ROOT = path.resolve(__dirname, "..");
const PROJECT = path.join(ROOT, "project");
const SITE_ORIGIN = "https://beava.dev";
const OG_IMAGE = `${SITE_ORIGIN}/assets/readme-banner.png`;

// ─────────────────────────────────────────────────────────────────────────
// Walk every index.html under project/, skipping the noisy directories.
// ─────────────────────────────────────────────────────────────────────────

const SKIP_DIRS = new Set(["_pagefind", "design-system", "node_modules"]);

function walk(dir, out = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isDirectory()) {
      if (SKIP_DIRS.has(entry.name)) continue;
      walk(path.join(dir, entry.name), out);
    } else if (entry.isFile() && entry.name === "index.html") {
      out.push(path.join(dir, entry.name));
    }
  }
  return out;
}

// project/foo/bar/index.html  ->  /foo/bar/
function urlPathFor(absFile) {
  const rel = path.relative(PROJECT, absFile).split(path.sep).join("/");
  if (rel === "index.html") return "/";
  return "/" + rel.replace(/index\.html$/, "");
}

// ─────────────────────────────────────────────────────────────────────────
// Inject pass — read each page, parse <title> + description, write back
// with OG / Twitter / canonical tags injected after the description.
// ─────────────────────────────────────────────────────────────────────────

function decode(html) {
  return html.replace(/&amp;/g, "&").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#39;/g, "'");
}

function escape(text) {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

const TITLE_RE = /<title>([^<]*)<\/title>/i;
const DESC_RE = /<meta\s+name="description"\s+content="([^"]*)"\s*\/?>/i;
const OG_PRESENT_RE = /<meta\s+property="og:title"/i;

function injectInto(html, url) {
  if (OG_PRESENT_RE.test(html)) return { html, skipped: true };

  const titleMatch = html.match(TITLE_RE);
  const descMatch = html.match(DESC_RE);
  if (!titleMatch || !descMatch) return { html, skipped: true };

  const title = decode(titleMatch[1]).trim();
  const desc = decode(descMatch[1]).trim();
  const canonical = SITE_ORIGIN + url;

  // Title shown in OG previews — drop the " · beava docs" / " — beava ..."
  // suffix so the headline is cleaner on a Twitter / Slack card.
  const ogTitle = title.replace(/\s*[—·]\s*beava.*$/i, "").trim() || title;

  const block = [
    `  <link rel="canonical" href="${canonical}">`,
    `  <meta property="og:site_name" content="beava">`,
    `  <meta property="og:type" content="website">`,
    `  <meta property="og:url" content="${canonical}">`,
    `  <meta property="og:title" content="${escape(ogTitle)}">`,
    `  <meta property="og:description" content="${escape(desc)}">`,
    `  <meta property="og:image" content="${OG_IMAGE}">`,
    `  <meta name="twitter:card" content="summary_large_image">`,
    `  <meta name="twitter:title" content="${escape(ogTitle)}">`,
    `  <meta name="twitter:description" content="${escape(desc)}">`,
    `  <meta name="twitter:image" content="${OG_IMAGE}">`,
  ].join("\n");

  // Inject right after the description tag.
  const next = html.replace(DESC_RE, (m) => `${m}\n${block}`);
  return { html: next, skipped: false };
}

// ─────────────────────────────────────────────────────────────────────────
// Sitemap + robots.txt
// ─────────────────────────────────────────────────────────────────────────

function lastmodFor(absFile) {
  try {
    const iso = execSync(`git log -1 --format=%cI -- "${absFile}"`, {
      stdio: ["ignore", "pipe", "ignore"],
      cwd: ROOT,
    })
      .toString()
      .trim();
    if (iso) return iso.slice(0, 10); // YYYY-MM-DD
  } catch (_) {}
  // Fallback: file mtime
  return new Date(fs.statSync(absFile).mtime).toISOString().slice(0, 10);
}

function priorityFor(url) {
  if (url === "/") return "1.0";
  if (url.startsWith("/docs/") && url.split("/").length <= 4) return "0.8";
  if (url.startsWith("/guide/") || url.startsWith("/sdk/")) return "0.7";
  return "0.5";
}

function writeSitemap(urls, files) {
  const entries = urls
    .map((u, i) => {
      const lastmod = lastmodFor(files[i]);
      return `  <url><loc>${SITE_ORIGIN}${u}</loc><lastmod>${lastmod}</lastmod><priority>${priorityFor(u)}</priority></url>`;
    })
    .join("\n");
  const xml = `<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${entries}
</urlset>
`;
  fs.writeFileSync(path.join(PROJECT, "sitemap.xml"), xml);
}

function writeRobots() {
  const body = `# Welcome, search engines. Crawl freely.
User-agent: *
Allow: /

# Internal artifacts not worth indexing.
Disallow: /_pagefind/
Disallow: /design-system/

Sitemap: ${SITE_ORIGIN}/sitemap.xml
`;
  fs.writeFileSync(path.join(PROJECT, "robots.txt"), body);
}

// ─────────────────────────────────────────────────────────────────────────
// Main
// ─────────────────────────────────────────────────────────────────────────

const files = walk(PROJECT);
let injected = 0;
let skipped = 0;
const indexedUrls = [];
const indexedFiles = [];

for (const file of files) {
  const url = urlPathFor(file);
  // Don't include /404.html or anything explicitly noindex in the sitemap.
  if (url === "/404.html" || url === "/404/") continue;
  indexedUrls.push(url);
  indexedFiles.push(file);

  const before = fs.readFileSync(file, "utf-8");
  const { html, skipped: wasSkipped } = injectInto(before, url);
  if (wasSkipped) {
    skipped++;
  } else if (html !== before) {
    fs.writeFileSync(file, html);
    injected++;
  }
}

writeSitemap(indexedUrls, indexedFiles);
writeRobots();

console.log(`SEO build: ${injected} pages injected, ${skipped} skipped (already had og:title or no title/description)`);
console.log(`  sitemap.xml: ${indexedUrls.length} URLs`);
console.log(`  robots.txt: written`);
