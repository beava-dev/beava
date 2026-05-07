// scripts/render-docs.mjs
//
// Phase 13.7 — markdown -> HTML converter for beava.dev docs site.
//
// Reads source markdown from <repo-root>/docs/*.md (Phase 13.0 spec docs),
// renders each into a page under <repo-root>/beava-website/project/docs/<route>/index.html
// matching the existing beava-website visual system (styles/colors_and_type.css + site.css).
//
// Reproducible: re-running produces byte-identical output.
// Idempotent: `npm run build:docs` twice -> zero git diffs.
//
// Pages are hand-rolled static HTML (NOT React+Babel) so Pagefind can index
// them directly via `addDirectory` at static build time.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import MarkdownIt from 'markdown-it';
import markdownItAnchor from 'markdown-it-anchor';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '../..');
const DOCS_SRC = path.join(REPO_ROOT, 'docs');
const DOCS_OUT = path.join(REPO_ROOT, 'beava-website/project/docs');
const CONFIG = JSON.parse(fs.readFileSync(path.join(__dirname, 'render-docs-config.json'), 'utf8'));

const WARNINGS_PATH = path.join(REPO_ROOT, 'render-docs-warnings.txt');
const warnings = [];

// ---------------------------------------------------------------
// Source-file discovery — every docs/*.md not in CONFIG.skip
// ---------------------------------------------------------------
function walk(dir, acc = []) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(p, acc);
    else if (entry.isFile() && p.endsWith('.md')) acc.push(p);
  }
  return acc;
}

function srcRel(absPath) {
  return path.relative(REPO_ROOT, absPath).replace(/\\/g, '/');
}

const skipSet = new Set((CONFIG.skip || []).map(s => s.replace(/\\/g, '/')));
const allMd = walk(DOCS_SRC).map(srcRel).sort();
const sourceMd = allMd.filter(p => !skipSet.has(p));

// ---------------------------------------------------------------
// Source -> route mapping
//
//   docs/index.md              -> /docs/                          -> docs/index.html
//   docs/quickstart.md         -> /docs/quickstart/               -> docs/quickstart/index.html
//   docs/sdk-api/python.md     -> /docs/sdk-api/python/           -> docs/sdk-api/python/index.html
//   docs/operators/index.md    -> /docs/operators/                -> docs/operators/index.html
//   docs/operators/core/index.md -> /docs/operators/core/         -> docs/operators/core/index.html
//   docs/operators/core/count.md -> /docs/operators/core/count/   -> docs/operators/core/count/index.html
// ---------------------------------------------------------------
function srcToRoute(srcRel) {
  // strip leading `docs/`
  const rel = srcRel.replace(/^docs\//, '');
  if (rel === 'index.md') return { route: '/docs/', outRel: 'index.html' };
  if (rel.endsWith('/index.md')) {
    const dir = rel.replace(/\/index\.md$/, '');
    return { route: `/docs/${dir}/`, outRel: `${dir}/index.html` };
  }
  // plain page -> wrap in dir/index.html for clean URLs
  const stem = rel.replace(/\.md$/, '');
  return { route: `/docs/${stem}/`, outRel: `${stem}/index.html` };
}

// Build the src->route map up-front so link rewriting can resolve targets.
const srcToRouteMap = new Map();
for (const src of sourceMd) {
  srcToRouteMap.set(src, srcToRoute(src));
}

// ---------------------------------------------------------------
// Title + description extraction
// ---------------------------------------------------------------
function extractTitle(md) {
  const m = md.match(/^#\s+(.+?)\s*$/m);
  return m ? m[1].trim() : 'Beava docs';
}

function extractDescription(md) {
  // First non-empty paragraph after the H1, ignoring blockquotes (`>` lead lines).
  const lines = md.split(/\r?\n/);
  let inH1 = false;
  let para = [];
  for (const line of lines) {
    if (/^#\s+/.test(line)) { inH1 = true; continue; }
    if (!inH1) continue;
    if (/^>/.test(line)) continue;
    if (/^#{2,}\s+/.test(line)) break; // next heading
    if (line.trim() === '') {
      if (para.length > 0) break;
      continue;
    }
    para.push(line.trim());
  }
  let txt = para.join(' ').replace(/\s+/g, ' ').trim();
  // strip markdown link syntax for clean meta description
  txt = txt.replace(/\[([^\]]+)\]\([^\)]+\)/g, '$1');
  txt = txt.replace(/`([^`]+)`/g, '$1');
  if (txt.length > 200) {
    const cut = txt.slice(0, 197).split(' ').slice(0, -1).join(' ');
    txt = cut + '…';
  }
  return txt || 'Beava — real-time feature server.';
}

function stripLeadingH1(md) {
  return md.replace(/^#\s+.+?\s*\n+/, '');
}

// ---------------------------------------------------------------
// markdown-it setup
// ---------------------------------------------------------------
const slugify = s => String(s).toLowerCase()
  .replace(/[^a-z0-9\s-]/g, '')
  .trim()
  .replace(/\s+/g, '-')
  .replace(/-+/g, '-');

const md = new MarkdownIt({
  html: false,
  linkify: true,
  typographer: false,
  breaks: false,
});
md.use(markdownItAnchor, {
  slugify,
  permalink: markdownItAnchor.permalink.linkInsideHeader({
    symbol: '#',
    placement: 'before',
    ariaHidden: true,
  }),
});

// ---------------------------------------------------------------
// Link rewriting — md links to .md files become site routes
//
// Rules:
//   ./quickstart.md                       -> /docs/quickstart/
//   ./operators/core/count.md             -> /docs/operators/core/count/
//   ../concepts/embed-mode.md             -> /docs/concepts/embed-mode/
//   ../examples/python/adtech.py          -> https://github.com/beava-dev/beava/blob/main/examples/python/adtech.py
//   ../.planning/decisions/ADR-001-...md  -> https://github.com/beava-dev/beava/blob/main/.planning/decisions/ADR-001-...md
//   #anchor                               -> #anchor (preserve)
//   http(s)://...                         -> http(s)://... (preserve)
// ---------------------------------------------------------------
const REPO_GH = 'https://github.com/beava-dev/beava/blob/main';

function rewriteLink(href, srcRel) {
  if (!href) return href;
  if (/^(https?:|mailto:|tel:)/.test(href)) return href;
  if (href.startsWith('#')) return href;

  // separate anchor
  const hashIdx = href.indexOf('#');
  let target = hashIdx >= 0 ? href.slice(0, hashIdx) : href;
  const anchor = hashIdx >= 0 ? href.slice(hashIdx) : '';

  // resolve relative to source file directory
  const srcDir = path.dirname(srcRel);
  const resolved = path.posix.normalize(path.posix.join(srcDir, target)).replace(/\\/g, '/');

  // strip leading ./ and any ../ that escaped above repo root (treated as repo root)
  let clean = resolved.replace(/^\.\//, '');
  while (clean.startsWith('../')) clean = clean.slice(3);

  // case 1: it points to a known docs/*.md source file
  if (srcToRouteMap.has(clean)) {
    return srcToRouteMap.get(clean).route + anchor;
  }
  // case 2: it points to docs/<...>/index.md (catch index variants)
  if (clean.endsWith('.md') && srcToRouteMap.has(clean)) {
    return srcToRouteMap.get(clean).route + anchor;
  }
  // case 3: cross-repo link to .planning / examples / crates / python / scripts
  // -> rewrite to GitHub blob URL (preserves clickability for now; tracked in warnings)
  if (/^(\.planning|examples|crates|python|scripts|src|benches|tests|launch|deploy|docs\/github-repo-surface-runbook\.md)\b/.test(clean)) {
    warnings.push(`${srcRel}: cross-repo link "${href}" -> ${REPO_GH}/${clean}${anchor}`);
    return `${REPO_GH}/${clean}${anchor}`;
  }
  // case 4: other docs/-relative .md that wasn't found in srcToRouteMap
  if (clean.startsWith('docs/') && clean.endsWith('.md')) {
    warnings.push(`${srcRel}: unresolved docs link "${href}" -> ${clean} (kept as-is)`);
    return href;
  }
  // case 5: default — keep as-is + warn
  warnings.push(`${srcRel}: unrecognized link target "${href}" (kept as-is)`);
  return href;
}

// markdown-it link rewriter
md.core.ruler.after('inline', 'rewrite-links', (state) => {
  const srcRel = state.env.srcRel;
  for (const block of state.tokens) {
    if (!block.children) continue;
    for (const t of block.children) {
      if (t.type === 'link_open') {
        const hrefIdx = t.attrIndex('href');
        if (hrefIdx >= 0) {
          t.attrs[hrefIdx][1] = rewriteLink(t.attrs[hrefIdx][1], srcRel);
        }
      }
      if (t.type === 'image') {
        const srcIdx = t.attrIndex('src');
        if (srcIdx >= 0) {
          const v = t.attrs[srcIdx][1];
          if (v.startsWith('./_assets/')) {
            // remap docs/_assets/foo.png -> /assets/foo.png (site assets dir)
            t.attrs[srcIdx][1] = '/assets/' + v.slice('./_assets/'.length);
          } else if (v.startsWith('_assets/')) {
            t.attrs[srcIdx][1] = '/assets/' + v.slice('_assets/'.length);
          } else if (!/^(https?:|\/)/.test(v)) {
            // leave other relative image refs alone but warn
            warnings.push(`${srcRel}: unhandled image src "${v}"`);
          }
          // attach loading="lazy" + responsive style
          if (t.attrIndex('loading') < 0) t.attrPush(['loading', 'lazy']);
          if (t.attrIndex('class') < 0) t.attrPush(['class', 'doc-img']);
        }
      }
    }
  }
});

// Harvest H2 entries for the right-rail TOC
function harvestToc(rendered) {
  const re = /<h2[^>]*\bid="([^"]+)"[^>]*>([\s\S]*?)<\/h2>/g;
  const out = [];
  let m;
  while ((m = re.exec(rendered)) !== null) {
    const text = m[2]
      .replace(/<a[^>]*aria-hidden[^>]*>[^<]*<\/a>/g, '')
      .replace(/<[^>]+>/g, '')
      .trim();
    out.push({ id: m[1], label: text });
  }
  return out;
}

// ---------------------------------------------------------------
// HTML escape for attribute values
// ---------------------------------------------------------------
function esc(s) {
  return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

// ---------------------------------------------------------------
// Sidebar HTML — flagged-active per page route
// ---------------------------------------------------------------
function renderSidebar(activeRoute) {
  const sections = CONFIG.sidebar.map(sec => {
    const items = sec.items.map(it => {
      const isActive = !it.external && it.href === activeRoute;
      const cls = isActive ? 'side-link active' : 'side-link';
      const target = it.external ? ' target="_blank" rel="noopener"' : '';
      return `      <li><a class="${cls}" href="${esc(it.href)}"${target}>${esc(it.label)}</a></li>`;
    }).join('\n');
    return `  <div class="side-section">
    <div class="side-title">${esc(sec.title)}</div>
    <ul class="side-list">
${items}
    </ul>
  </div>`;
  }).join('\n');
  return `<nav class="docs-sidebar" aria-label="Docs navigation">\n${sections}\n</nav>`;
}

// ---------------------------------------------------------------
// TOC HTML — right-rail
// ---------------------------------------------------------------
function renderToc(toc) {
  if (!toc || toc.length === 0) return '<aside class="docs-toc" aria-label="On this page"></aside>';
  const items = toc.map(t => `      <li><a href="#${esc(t.id)}">${esc(t.label)}</a></li>`).join('\n');
  return `<aside class="docs-toc" aria-label="On this page">
  <div class="toc-title">On this page</div>
  <ul class="toc-list">
${items}
  </ul>
</aside>`;
}

// ---------------------------------------------------------------
// Site nav (header) — minimal hand-rolled to avoid pulling Shared.jsx
// ---------------------------------------------------------------
function renderNav() {
  return `<header class="site-nav">
  <div class="nav-inner">
    <a class="brand" href="/"><img src="/assets/logo-mark.png" alt="" width="36" height="36"/><span>beava</span></a>
    <ul class="nav-links">
      <li><a href="/guide/">Guide</a></li>
      <li><a href="/docs/" aria-current="page">Docs</a></li>
      <li><a href="https://github.com/beava-dev/beava" target="_blank" rel="noopener">GitHub</a></li>
    </ul>
    <div class="nav-search" id="search"></div>
  </div>
</header>
<link rel="stylesheet" href="/_pagefind/pagefind-ui.css">
<script src="/_pagefind/pagefind-ui.js" defer></script>
<script>
  window.addEventListener('DOMContentLoaded', function () {
    if (window.PagefindUI) {
      new PagefindUI({ element: '#search', showSubResults: true, resetStyles: false, showImages: false });
    }
  });
</script>`;
}

function renderFooter() {
  return `<footer class="site-foot">
  <div class="foot-inner">
    <span>© 2026 beava labs · Apache 2.0</span>
    <a href="https://github.com/beava-dev/beava" target="_blank" rel="noopener">GitHub</a>
  </div>
</footer>`;
}

// ---------------------------------------------------------------
// Breadcrumbs — derived from route
// ---------------------------------------------------------------
// Set of routes for which we have an index page on disk (computed once).
const knownRoutes = new Set([...srcToRouteMap.values()].map(v => v.route));

function renderCrumbs(route, title) {
  const parts = route.replace(/^\/+|\/+$/g, '').split('/').filter(Boolean);
  const crumbs = [];
  let acc = '';
  for (let i = 0; i < parts.length; i++) {
    acc = acc + '/' + parts[i];
    const hrefAcc = acc + '/';
    const last = i === parts.length - 1;
    const label = last ? title : parts[i].replace(/-/g, ' ').replace(/\b\w/g, c => c.toUpperCase());
    if (last) {
      crumbs.push(`<span class="crumb-cur">${esc(label)}</span>`);
    } else if (knownRoutes.has(hrefAcc)) {
      crumbs.push(`<a href="${esc(hrefAcc)}">${esc(label)}</a>`);
    } else {
      crumbs.push(`<span>${esc(label)}</span>`);
    }
  }
  return `<nav class="crumbs" aria-label="Breadcrumb">${crumbs.join('<span class="crumb-sep">›</span>')}</nav>`;
}

// ---------------------------------------------------------------
// Page shell
// ---------------------------------------------------------------
function renderPage({ title, description, route, body, toc, srcRel }) {
  // Compute relative depth for canonical link (always absolute) — we use
  // absolute /styles/... paths so depth doesn't matter.
  const sidebar = renderSidebar(route);
  const tocHtml = renderToc(toc);
  const nav = renderNav();
  const foot = renderFooter();
  const crumbs = renderCrumbs(route, title);

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${esc(title)} — beava docs</title>
<meta name="description" content="${esc(description)}">
<link rel="canonical" href="${esc(CONFIG.siteUrl + route)}">
<link rel="icon" type="image/png" sizes="32x32" href="/assets/favicon-32.png">
<link rel="icon" type="image/png" sizes="16x16" href="/assets/favicon-16.png">
<link rel="apple-touch-icon" sizes="180x180" href="/assets/apple-touch-icon.png">
<link rel="shortcut icon" href="/assets/favicon.ico">
<link rel="stylesheet" href="/styles/colors_and_type.css">
<link rel="stylesheet" href="/styles/site.css">
<link rel="stylesheet" href="/styles/docs.css">
<meta name="docs-source" content="${esc(srcRel)}">
</head>
<body class="beava docs-body">
${nav}
<div class="docs-shell">
${sidebar}
<main class="docs-main" data-pagefind-body>
${crumbs}
<article class="docs-prose">
<h1>${esc(title)}</h1>
${body}
</article>
</main>
${tocHtml}
</div>
${foot}
</body>
</html>
`;
}

// ---------------------------------------------------------------
// Main render loop
// ---------------------------------------------------------------
function render(srcRel) {
  const absSrc = path.join(REPO_ROOT, srcRel);
  const raw = fs.readFileSync(absSrc, 'utf8');
  const title = extractTitle(raw);
  const description = extractDescription(raw);
  const stripped = stripLeadingH1(raw);
  const env = { srcRel };
  const bodyHtml = md.render(stripped, env);
  const toc = harvestToc(bodyHtml);
  const { route, outRel } = srcToRouteMap.get(srcRel);
  const html = renderPage({ title, description, route, body: bodyHtml, toc, srcRel });
  const outAbs = path.join(DOCS_OUT, outRel);
  fs.mkdirSync(path.dirname(outAbs), { recursive: true });
  fs.writeFileSync(outAbs, html);
  return { route, outRel, title, tocLen: toc.length };
}

let pages = 0;
let failed = 0;
const rendered = [];
for (const src of sourceMd) {
  try {
    const r = render(src);
    rendered.push(r);
    pages++;
  } catch (err) {
    failed++;
    console.error(`render failed: ${src}: ${err.message}`);
  }
}

// Write warnings file (overwrite each run; used by Plan 02 link audit)
if (warnings.length > 0) {
  fs.writeFileSync(WARNINGS_PATH, warnings.join('\n') + '\n');
} else if (fs.existsSync(WARNINGS_PATH)) {
  fs.unlinkSync(WARNINGS_PATH);
}

console.log(`render-docs: ${pages} pages rendered, ${failed} failed, ${warnings.length} link warnings`);
if (failed > 0) process.exit(1);
