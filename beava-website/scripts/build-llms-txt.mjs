// scripts/build-llms-txt.mjs
//
// Generate /sdk/llms.txt — a plain-text, markdown-flavored
// concatenation of all 11 SDK reference pages. Optimized for
// LLM agents that want the full SDK surface in one fetch
// without parsing site chrome / JS.
//
// Output: beava-website/project/sdk/llms.txt
//
// Run: cd beava-website && node scripts/build-llms-txt.mjs

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '../..');
const SITE_ROOT = path.join(REPO_ROOT, 'beava-website/project');
const OUT_PATH  = path.join(SITE_ROOT, 'sdk/llms.txt');

// Ordered list — same as the SdkSidebar nav config so the text version
// reads in the same logical order as a user would click through.
const PAGES = [
  { url: '/sdk/python/',              file: 'sdk/python/index.html',           title: 'Quickstart' },
  { url: '/sdk/python/app/',          file: 'sdk/python/app/index.html',       title: 'App client (bv.App)' },
  { url: '/sdk/python/event/',        file: 'sdk/python/event/index.html',     title: '@bv.event' },
  { url: '/sdk/python/table/',        file: 'sdk/python/table/index.html',     title: '@bv.table' },
  { url: '/sdk/python/col-lit/',      file: 'sdk/python/col-lit/index.html',   title: 'bv.col / bv.lit' },
  { url: '/sdk/python/operators/',    file: 'sdk/python/operators/index.html', title: 'Operator catalogue' },
  { url: '/sdk/python/errors/',       file: 'sdk/python/errors/index.html',    title: 'Errors' },
  { url: '/sdk/http/push/',           file: 'sdk/http/push/index.html',        title: 'POST /push' },
  { url: '/sdk/http/get/',            file: 'sdk/http/get/index.html',         title: 'POST /get' },
  { url: '/sdk/http/register/',       file: 'sdk/http/register/index.html',    title: 'POST /register' },
  { url: '/sdk/http/wire-spec/',      file: 'sdk/http/wire-spec/index.html',   title: 'Wire spec' },
];

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

function main() {
  const out = [];

  out.push('# Beava SDK reference — full plain-text version');
  out.push('');
  out.push('This is a concatenation of all 11 SDK reference pages on beava.dev');
  out.push('formatted for LLM agents that want the full SDK surface in one fetch.');
  out.push('Source URLs are noted at the top of each section.');
  out.push('');
  out.push('Generated by beava-website/scripts/build-llms-txt.mjs from the canonical');
  out.push('HTML pages under beava-website/project/sdk/. To regenerate after page');
  out.push('edits: `cd beava-website && node scripts/build-llms-txt.mjs`.');
  out.push('');
  out.push(`Total pages: ${PAGES.length}`);
  out.push('');
  out.push('## Index');
  out.push('');
  for (const p of PAGES) {
    out.push(`- ${p.title} — https://beava.dev${p.url}`);
  }

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
    const md = htmlToMarkdown(main);

    out.push(SEPARATOR.trim());
    out.push('');
    out.push(`# ${p.title}`);
    out.push(`Source: https://beava.dev${p.url}`);
    out.push('');
    out.push(md);
  }

  const text = out.join('\n').replace(/\n{3,}/g, '\n\n').trim() + '\n';
  fs.writeFileSync(OUT_PATH, text);
  console.log(`build-llms-txt: wrote ${OUT_PATH} (${text.length.toLocaleString()} chars, ${text.split('\n').length.toLocaleString()} lines)`);
}

main();
