// scripts/build-llms-txt.mjs
//
// Generate the two-tier llms.txt artifacts for the SDK reference.
// Follows the llmstxt.org convention:
//
//   TIER 1 — /sdk/llms.txt
//     Short markdown index. One link + one-line summary per page,
//     grouped by sidebar section. Cheap to ingest; routes agents to
//     the resource they need.
//
//   TIER 2 — /sdk/llms-full.txt
//     Full plain-text concatenation of all SDK pages, markdown-
//     flavored. ~145 KB. For agents that want the entire SDK surface
//     in one fetch without parsing site chrome / JS.
//
// Run: cd beava-website && node scripts/build-llms-txt.mjs

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '../..');
const SITE_ROOT = path.join(REPO_ROOT, 'beava-website/project');
const OUT_INDEX = path.join(SITE_ROOT, 'sdk/llms.txt');
const OUT_FULL  = path.join(SITE_ROOT, 'sdk/llms-full.txt');

// Ordered list — same as the SdkSidebar nav config so the text version
// reads in the same logical order as a user would click through.
// `section` groups pages for the index file's H2 headings.
const PAGES = [
  { section: 'Start here',  url: '/sdk/python/',              file: 'sdk/python/index.html',           title: 'Quickstart' },
  { section: 'Server',      url: '/sdk/server/',              file: 'sdk/server/index.html',           title: 'Server configuration' },
  { section: 'Python SDK',  url: '/sdk/python/app/',          file: 'sdk/python/app/index.html',       title: 'App client (bv.App)' },
  { section: 'Python SDK',  url: '/sdk/python/event/',        file: 'sdk/python/event/index.html',     title: '@bv.event' },
  { section: 'Python SDK',  url: '/sdk/python/table/',        file: 'sdk/python/table/index.html',     title: '@bv.table' },
  { section: 'Python SDK',  url: '/sdk/python/col-lit/',      file: 'sdk/python/col-lit/index.html',   title: 'bv.col / bv.lit' },
  { section: 'Python SDK',  url: '/sdk/python/operators/',    file: 'sdk/python/operators/index.html', title: 'Operator catalogue' },
  { section: 'Python SDK',  url: '/sdk/python/errors/',       file: 'sdk/python/errors/index.html',    title: 'Errors' },
  { section: 'HTTP API',    url: '/sdk/http/push/',           file: 'sdk/http/push/index.html',        title: 'POST /push' },
  { section: 'HTTP API',    url: '/sdk/http/get/',            file: 'sdk/http/get/index.html',         title: 'POST /get' },
  { section: 'HTTP API',    url: '/sdk/http/register/',       file: 'sdk/http/register/index.html',    title: 'POST /register' },
  { section: 'HTTP API',    url: '/sdk/http/wire-spec/',      file: 'sdk/http/wire-spec/index.html',   title: 'Wire spec' },
];

// Pull <meta name="description"> from a page — used as the one-line
// summary in the tier-1 index.
function extractMetaDescription(html) {
  const m = html.match(/<meta\s+name="description"\s+content="([^"]+)"\s*\/?>/i);
  return m ? m[1] : '';
}

// Pull all h2[id] from <main> for the per-page anchor list shown in
// the tier-1 index. Helps agents jump directly to relevant section.
function extractH2Anchors(mainHtml) {
  const out = [];
  const re = /<h2\s+id="([^"]+)"[^>]*>([\s\S]*?)<\/h2>/gi;
  let m;
  while ((m = re.exec(mainHtml)) !== null) {
    const text = m[2].replace(/<[^>]+>/g, '').replace(/\s+/g, ' ').trim();
    if (text) out.push({ id: m[1], text });
  }
  return out;
}

// Decode common HTML entities. Keep narrow — we only need the ones
// that actually appear in the source pages.
function decodeEntities(s) {
  return s
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&')
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&nbsp;/g, ' ')
    .replace(/&mdash;/g, '—')
    .replace(/&ndash;/g, '–')
    .replace(/&hellip;/g, '…')
    .replace(/&rarr;/g, '→')
    .replace(/&larr;/g, '←')
    .replace(/&times;/g, '×')
    .replace(/&middot;/g, '·')
    .replace(/&laquo;/g, '«')
    .replace(/&raquo;/g, '»')
    .replace(/&#(\d+);/g, (_, n) => String.fromCharCode(parseInt(n, 10)));
}

// Extract <main class="content" ...>...</main>. Pages all use this
// shape exactly; if the regex misses, surface and skip.
function extractMain(html) {
  const m = html.match(/<main[^>]*class="content"[^>]*>([\s\S]*?)<\/main>/);
  return m ? m[1] : null;
}

// Strip <script> / <style> / <svg> blocks even if inside main, plus
// the <div class="crumbs">…</div> nav strip (noisy and redundant
// with the per-section "Source:" header we add ourselves).
function stripScripts(s) {
  return s
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/<style[\s\S]*?<\/style>/gi, '')
    .replace(/<svg[\s\S]*?<\/svg>/gi, '')
    .replace(/<div[^>]*class="crumbs"[^>]*>[\s\S]*?<\/div>/gi, '')
    .replace(/<header[^>]*class="hero"[^>]*>[\s\S]*?<\/header>/gi, (block) => {
      // Keep the hero's h1 + lede; drop the eyebrow + mascot SVG noise.
      const h1   = (block.match(/<h1[^>]*>([\s\S]*?)<\/h1>/i)        || [, ''])[1];
      const lede = (block.match(/<p[^>]*class="lede"[^>]*>([\s\S]*?)<\/p>/i) || [, ''])[1];
      return `<h1>${h1}</h1><p>${lede}</p>`;
    })
    .replace(/<header[^>]*class="ref-hero"[^>]*>[\s\S]*?<\/header>/gi, (block) => {
      const h1   = (block.match(/<h1[^>]*>([\s\S]*?)<\/h1>/i)        || [, ''])[1];
      const lede = (block.match(/<p[^>]*class="lede"[^>]*>([\s\S]*?)<\/p>/i) || [, ''])[1];
      return `<h1>${h1}</h1><p>${lede}</p>`;
    })
    // Drop the bottom feedback widget (.feedback) and pager mount div.
    .replace(/<div[^>]*class="feedback"[^>]*>[\s\S]*?<\/div>\s*<\/div>/gi, '')
    .replace(/<div[^>]*id="bv-sdk-pager"[^>]*>[\s\S]*?<\/div>/gi, '');
}

// Convert headings to markdown atx form.
function rewriteHeadings(s) {
  return s
    .replace(/<h1[^>]*>([\s\S]*?)<\/h1>/gi, (_, t) => `\n\n# ${stripTags(t).trim()}\n`)
    .replace(/<h2[^>]*>([\s\S]*?)<\/h2>/gi, (_, t) => `\n\n## ${stripTags(t).trim()}\n`)
    .replace(/<h3[^>]*>([\s\S]*?)<\/h3>/gi, (_, t) => `\n\n### ${stripTags(t).trim()}\n`)
    .replace(/<h4[^>]*>([\s\S]*?)<\/h4>/gi, (_, t) => `\n\n#### ${stripTags(t).trim()}\n`);
}

// Turn <pre>...</pre> blocks into fenced code. Inside <pre>, drop
// any inner spans (syntax-highlight wrappers) but keep their text.
function rewriteCodeBlocks(s) {
  return s.replace(/<pre[^>]*>([\s\S]*?)<\/pre>/gi, (_, body) => {
    const code = stripTags(body).replace(/\n+$/, '');
    return `\n\n\`\`\`\n${decodeEntities(code)}\n\`\`\`\n`;
  });
}

// <code>x</code> inline → `x`. Run AFTER pre rewrite so we don't
// double-wrap content already inside fenced blocks.
function rewriteInlineCode(s) {
  return s.replace(/<code[^>]*>([\s\S]*?)<\/code>/gi, (_, t) => {
    const inner = stripTags(t);
    return `\`${decodeEntities(inner)}\``;
  });
}

// Bullet + ordered list rewrites.
function rewriteLists(s) {
  return s
    .replace(/<li[^>]*>([\s\S]*?)<\/li>/gi, (_, t) => `\n- ${stripTags(t).trim()}`)
    .replace(/<\/?(ul|ol)[^>]*>/gi, '\n');
}

// <strong>/<b>/<em>/<i> → markdown inline emphasis.
function rewriteEmphasis(s) {
  return s
    .replace(/<(strong|b)[^>]*>([\s\S]*?)<\/\1>/gi, (_, _t, body) => `**${stripTags(body)}**`)
    .replace(/<(em|i)[^>]*>([\s\S]*?)<\/\1>/gi, (_, _t, body) => `*${stripTags(body)}*`);
}

// Paragraph + line break tightening.
function rewriteParagraphs(s) {
  return s
    .replace(/<p[^>]*>/gi, '\n\n')
    .replace(/<\/p>/gi, '')
    .replace(/<br\s*\/?>/gi, '\n');
}

// Cards (<a class="card">...</a>) — keep title + description + url.
function rewriteCards(s) {
  return s.replace(/<a[^>]*class="card"[^>]*href="([^"]+)"[^>]*>([\s\S]*?)<\/a>/gi,
    (_, href, body) => {
      const ttl = (body.match(/<div[^>]*class="ttl"[^>]*>([\s\S]*?)<\/div>/i) || [, ''])[1];
      const desc = (body.match(/<p[^>]*class="desc"[^>]*>([\s\S]*?)<\/p>/i) || [, ''])[1];
      return `\n\n→ **${stripTags(ttl).trim()}** (${href}) — ${stripTags(desc).trim()}\n`;
    });
}

// Generic anchor — keep text + url.
function rewriteLinks(s) {
  return s.replace(/<a[^>]*href="([^"]+)"[^>]*>([\s\S]*?)<\/a>/gi, (_, href, body) => {
    const text = stripTags(body).trim();
    if (!text) return '';
    if (text === href) return text;
    // Skip noisy in-page anchor-copy links.
    return `${text} (${href})`;
  });
}

// Drop callout / accordion / tab / param / method-head wrapper divs
// so their contents flow into the prose stream.
function rewriteDivs(s) {
  return s
    .replace(/<\/(div|aside|section|header|footer|article|details|summary|span)[^>]*>/gi, '')
    .replace(/<(div|aside|section|header|footer|article|details|summary|span)[^>]*>/gi, '');
}

// Tables — convert to a minimal markdown form (best effort; rare on
// SDK pages but used in operator alias table + opcode tables).
function rewriteTables(s) {
  return s.replace(/<table[\s\S]*?<\/table>/gi, (block) => {
    const rows = [];
    const rowRe = /<tr[^>]*>([\s\S]*?)<\/tr>/gi;
    let m;
    while ((m = rowRe.exec(block)) !== null) {
      const cellRe = /<(?:td|th)[^>]*>([\s\S]*?)<\/(?:td|th)>/gi;
      const cells = [];
      let c;
      while ((c = cellRe.exec(m[1])) !== null) {
        cells.push(stripTags(c[1]).replace(/\s+/g, ' ').trim());
      }
      if (cells.length) rows.push('| ' + cells.join(' | ') + ' |');
    }
    if (rows.length === 0) return '';
    // Insert a markdown header-divider after the first row.
    if (rows.length >= 1) {
      const colCount = rows[0].split('|').length - 2;
      rows.splice(1, 0, '| ' + Array(colCount).fill('---').join(' | ') + ' |');
    }
    return '\n\n' + rows.join('\n') + '\n\n';
  });
}

// Final tag strip — anything left.
function stripTags(s) {
  return s.replace(/<[^>]+>/g, '');
}

// Normalize whitespace.
function tidy(s) {
  return decodeEntities(s)
    .replace(/[ \t]+\n/g, '\n')        // trailing space on lines
    .replace(/\n{3,}/g, '\n\n')        // collapse 3+ blank lines
    .replace(/^\s+|\s+$/g, '')         // trim
    + '\n';
}

function htmlToMarkdown(html) {
  let s = html;
  s = stripScripts(s);
  s = rewriteTables(s);          // before generic divs strip
  s = rewriteCodeBlocks(s);      // before inline-code so <code> inside <pre> isn't double-wrapped (pre body is already stripped of tags)
  s = rewriteCards(s);           // before generic <a> rewrite
  s = rewriteLinks(s);
  s = rewriteHeadings(s);
  s = rewriteLists(s);
  s = rewriteEmphasis(s);
  s = rewriteParagraphs(s);
  s = rewriteInlineCode(s);
  s = rewriteDivs(s);
  s = stripTags(s);
  return tidy(s);
}

const SEPARATOR = '\n\n' + '='.repeat(72) + '\n\n';

// Read each page once; collect both the markdown body (for the full
// dump) and metadata (for the index).
function loadPages() {
  const enriched = [];
  for (const p of PAGES) {
    const fp = path.join(SITE_ROOT, p.file);
    if (!fs.existsSync(fp)) {
      console.warn(`SKIP missing: ${fp}`);
      continue;
    }
    const html = fs.readFileSync(fp, 'utf8');
    const main = extractMain(html);
    if (!main) {
      console.warn(`SKIP no <main class="content">: ${fp}`);
      continue;
    }
    enriched.push({
      ...p,
      description: extractMetaDescription(html),
      anchors: extractH2Anchors(main),
      markdown: htmlToMarkdown(main),
    });
  }
  return enriched;
}

// TIER 1 — the short index. llmstxt.org-shaped: H1 + lede, then
// section H2s, each with one bullet per page (markdown-link + summary
// + bracketed h2-anchor list so agents can deep-link).
function buildIndex(pages, fullPath) {
  const out = [];
  out.push('# Beava SDK reference');
  out.push('');
  out.push('> Beava is a single-binary real-time feature server for fraud, ad-tech, and behavioral analytics. This is the SDK reference: boot configuration, Python SDK, HTTP API, and wire spec.');
  out.push('');
  out.push('This is the **tier-1 index** (short, structured). For the full plain-text concatenation of every SDK page, see [llms-full.txt](https://beava.dev/sdk/llms-full.txt).');
  out.push('');
  out.push('Generated by `beava-website/scripts/build-llms-txt.mjs` from the canonical HTML pages under `beava-website/project/sdk/`. Regenerate with `npm run build:llms` after page edits.');
  out.push('');

  // Group pages by section, preserving definition order.
  const sectionOrder = [];
  const bySection = new Map();
  for (const p of pages) {
    if (!bySection.has(p.section)) {
      bySection.set(p.section, []);
      sectionOrder.push(p.section);
    }
    bySection.get(p.section).push(p);
  }

  for (const sec of sectionOrder) {
    out.push(`## ${sec}`);
    out.push('');
    for (const p of bySection.get(sec)) {
      const desc = p.description || '(no description)';
      out.push(`- [${p.title}](https://beava.dev${p.url}): ${desc}`);
      if (p.anchors.length > 0) {
        const list = p.anchors.map(a => `\`#${a.id}\` ${a.text}`).join(' · ');
        out.push(`  Sections: ${list}`);
      }
    }
    out.push('');
  }

  out.push('## Full content');
  out.push('');
  out.push('- [llms-full.txt](https://beava.dev/sdk/llms-full.txt): All SDK pages concatenated as plain text with markdown headings + fenced code blocks. ~145 KB. Single fetch covers the full surface.');
  out.push('');

  return out.join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n';
}

// TIER 2 — the full dump. Each page rendered as markdown, separated
// by horizontal rules. Same structure as before; just renamed.
function buildFull(pages) {
  const out = [];
  out.push('# Beava SDK reference — full plain-text version');
  out.push('');
  out.push('Concatenation of all SDK reference pages on beava.dev formatted');
  out.push('for LLM agents that want the full surface in one fetch.');
  out.push('');
  out.push('See also the short structured index at https://beava.dev/sdk/llms.txt');
  out.push('');
  out.push('Generated by `beava-website/scripts/build-llms-txt.mjs` from the canonical');
  out.push('HTML pages under `beava-website/project/sdk/`. Regenerate with');
  out.push('`npm run build:llms` after page edits.');
  out.push('');
  out.push(`Total pages: ${pages.length}`);
  out.push('');
  out.push('## Index');
  out.push('');
  for (const p of pages) {
    out.push(`- ${p.title} — https://beava.dev${p.url}`);
  }

  for (const p of pages) {
    out.push(SEPARATOR.trim());
    out.push('');
    out.push(`# ${p.title}`);
    out.push(`Source: https://beava.dev${p.url}`);
    out.push('');
    out.push(p.markdown);
  }

  return out.join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n';
}

function writeWithLog(filePath, body) {
  fs.writeFileSync(filePath, body);
  const lines = body.split('\n').length;
  console.log(`  wrote ${path.relative(REPO_ROOT, filePath)}` +
              ` (${body.length.toLocaleString()} chars, ${lines.toLocaleString()} lines)`);
}

function main() {
  const pages = loadPages();
  console.log(`build-llms-txt: ${pages.length} pages loaded`);
  writeWithLog(OUT_INDEX, buildIndex(pages, OUT_FULL));
  writeWithLog(OUT_FULL,  buildFull(pages));
  console.log('build-llms-txt: done');
}

main();
