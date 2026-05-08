// scripts/build-mintlify-docs.mjs
//
// Converts the SDK reference + concepts pages from
// beava-website/project/sdk/ and beava-website/project/docs/concepts/
// into Mintlify-shaped markdown files under repo-root /docs/.
//
// Reuses the HTML-to-markdown logic from
// beava-website/scripts/build-llms-txt.mjs.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '..');
const SITE_ROOT = path.join(REPO_ROOT, 'beava-website/project');
const OUT_ROOT  = path.join(REPO_ROOT, 'docs');

// Source page → Mintlify slug. Sidebar / TOC / pager are stripped via
// `extractMain`; chrome-only divs (.crumbs, .ref-hero, .footer-area,
// .feedback) are dropped before HTML→MD conversion.
const PAGES = [
  // Get started
  { src: 'sdk/python/index.html',           slug: 'quickstart',                 title: 'Quickstart',                  section: 'Get started', desc: 'Install beava, declare a feature, push events, query — in five minutes.' },
  { src: 'sdk/server/index.html',           slug: 'server-config',              title: 'Server configuration',        section: 'Get started', desc: 'CLI flags, environment variables, YAML config, defaults.' },

  // Concepts (carry the website concept pages over)
  { src: 'docs/concepts/streams/index.html',   slug: 'concepts/streams',     title: 'Streams',     section: 'Concepts' },
  { src: 'docs/concepts/tables/index.html',    slug: 'concepts/tables',      title: 'Tables',      section: 'Concepts' },
  { src: 'docs/concepts/windows/index.html',   slug: 'concepts/windows',     title: 'Windows',     section: 'Concepts' },
  { src: 'docs/concepts/freshness/index.html', slug: 'concepts/freshness',   title: 'Freshness',   section: 'Concepts' },
  { src: 'docs/concepts/get-and-batch-get/index.html', slug: 'concepts/get-and-batch-get', title: 'Get and batch_get', section: 'Concepts' },

  // Python SDK
  { src: 'sdk/python/app/index.html',       slug: 'python/app',                 title: 'App client',                  section: 'Python SDK', desc: 'bv.App() — the synchronous Python client. Seven wire-mapped methods.' },
  { src: 'sdk/python/event/index.html',     slug: 'python/event',               title: '@bv.event',                   section: 'Python SDK', desc: 'Declare event sources and derivations.' },
  { src: 'sdk/python/table/index.html',     slug: 'python/table',               title: '@bv.table',                   section: 'Python SDK', desc: 'Declare per-entity feature tables.' },
  { src: 'sdk/python/col-lit/index.html',   slug: 'python/col-lit',             title: 'bv.col / bv.lit',             section: 'Python SDK', desc: 'Column references and literal values for the chain DSL.' },
  { src: 'sdk/python/operators/index.html', slug: 'python/operators',           title: 'Operator catalogue',          section: 'Python SDK', desc: '54 aggregation primitives across seven families.' },
  { src: 'sdk/python/errors/index.html',    slug: 'python/errors',              title: 'Errors',                      section: 'Python SDK', desc: 'Python exceptions and wire error codes.' },

  // HTTP API
  { src: 'sdk/http/push/index.html',        slug: 'http/push',                  title: 'POST /push',                  section: 'HTTP API',   desc: 'Push a single event.' },
  { src: 'sdk/http/get/index.html',         slug: 'http/get',                   title: 'POST /get',                   section: 'HTTP API',   desc: 'Read a feature row by entity key.' },
  { src: 'sdk/http/register/index.html',    slug: 'http/register',              title: 'POST /register',              section: 'HTTP API',   desc: 'Register event sources and derivations.' },
  { src: 'sdk/http/wire-spec/index.html',   slug: 'http/wire-spec',             title: 'Wire spec',                   section: 'HTTP API',   desc: 'TCP frame format and HTTP route table.' },
];

// ─── HTML → markdown ────────────────────────────────────────────

function decodeEntities(s) {
  return s
    .replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&')
    .replace(/&quot;/g, '"').replace(/&apos;/g, "'").replace(/&nbsp;/g, ' ')
    .replace(/&mdash;/g, '—').replace(/&ndash;/g, '–').replace(/&hellip;/g, '…')
    .replace(/&rarr;/g, '→').replace(/&larr;/g, '←').replace(/&times;/g, '×')
    .replace(/&middot;/g, '·').replace(/&laquo;/g, '«').replace(/&raquo;/g, '»')
    .replace(/&rsquo;/g, '’').replace(/&lsquo;/g, '‘')
    .replace(/&rdquo;/g, '”').replace(/&ldquo;/g, '“')
    .replace(/&#(\d+);/g, (_, n) => String.fromCharCode(parseInt(n, 10)));
}

function extractMain(html) {
  // Try sdk-style <main class="content"> first; fall back to <main className="bv-content">.
  let m = html.match(/<main[^>]*class="content"[^>]*>([\s\S]*?)<\/main>/);
  if (m) return m[1];
  m = html.match(/<main[^>]*className="bv-content"[^>]*>([\s\S]*?)<\/main>/);
  return m ? m[1] : null;
}

function stripChrome(s) {
  return s
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/<style[\s\S]*?<\/style>/gi, '')
    .replace(/<svg[\s\S]*?<\/svg>/gi, '')
    .replace(/<div[^>]*class="(crumbs|bv-crumbs)"[^>]*>[\s\S]*?<\/div>/gi, '')
    .replace(/<header[^>]*class="(ref-hero|hero)"[^>]*>([\s\S]*?)<\/header>/gi, (block) => {
      const h1   = (block.match(/<h1[^>]*>([\s\S]*?)<\/h1>/i)        || [, ''])[1];
      const lede = (block.match(/<p[^>]*class="lede"[^>]*>([\s\S]*?)<\/p>/i) || [, ''])[1];
      return `<h1>${h1}</h1><p>${lede}</p>`;
    })
    .replace(/<div[^>]*class="(footer-area|feedback|docs-help-callout)"[^>]*>[\s\S]*?<\/div>\s*<\/div>/gi, '')
    .replace(/<div[^>]*id="bv-sdk-pager"[^>]*>[\s\S]*?<\/div>/gi, '');
}

function stripTags(s) { return s.replace(/<[^>]+>/g, ''); }

function rewriteCodeBlocks(s) {
  return s.replace(/<pre[^>]*>([\s\S]*?)<\/pre>/gi, (_, body) => {
    const code = stripTags(body).replace(/\n+$/, '');
    return `\n\n\`\`\`\n${decodeEntities(code)}\n\`\`\`\n`;
  });
}

function rewriteCards(s) {
  return s.replace(/<a[^>]*class="card"[^>]*href="([^"]+)"[^>]*>([\s\S]*?)<\/a>/gi,
    (_, href, body) => {
      const ttl  = (body.match(/<div[^>]*class="ttl"[^>]*>([\s\S]*?)<\/div>/i) || [, ''])[1];
      const desc = (body.match(/<p[^>]*class="desc"[^>]*>([\s\S]*?)<\/p>/i)    || [, ''])[1];
      return `\n\n→ **${stripTags(ttl).trim()}** ([${href}](${href})) — ${stripTags(desc).trim()}\n`;
    });
}

function rewriteLinks(s) {
  return s.replace(/<a[^>]*href="([^"]+)"[^>]*>([\s\S]*?)<\/a>/gi, (_, href, body) => {
    const text = stripTags(body).trim();
    if (!text) return '';
    if (text === href) return `<${href}>`;
    return `[${text}](${href})`;
  });
}

function rewriteHeadings(s) {
  return s
    .replace(/<h1[^>]*>([\s\S]*?)<\/h1>/gi, (_, t) => `\n\n# ${stripTags(t).trim()}\n`)
    .replace(/<h2[^>]*>([\s\S]*?)<\/h2>/gi, (_, t) => `\n\n## ${stripTags(t).trim()}\n`)
    .replace(/<h3[^>]*>([\s\S]*?)<\/h3>/gi, (_, t) => `\n\n### ${stripTags(t).trim()}\n`)
    .replace(/<h4[^>]*>([\s\S]*?)<\/h4>/gi, (_, t) => `\n\n#### ${stripTags(t).trim()}\n`);
}

function rewriteLists(s) {
  return s
    .replace(/<li[^>]*>([\s\S]*?)<\/li>/gi, (_, t) => `\n- ${stripTags(t).trim()}`)
    .replace(/<\/?(ul|ol)[^>]*>/gi, '\n');
}

function rewriteEmphasis(s) {
  return s
    .replace(/<(strong|b)[^>]*>([\s\S]*?)<\/\1>/gi, (_, _t, body) => `**${stripTags(body)}**`)
    .replace(/<(em|i)[^>]*>([\s\S]*?)<\/\1>/gi, (_, _t, body) => `*${stripTags(body)}*`);
}

function rewriteParagraphs(s) {
  return s.replace(/<p[^>]*>/gi, '\n\n').replace(/<\/p>/gi, '').replace(/<br\s*\/?>/gi, '\n');
}

function rewriteInlineCode(s) {
  return s.replace(/<code[^>]*>([\s\S]*?)<\/code>/gi, (_, t) => `\`${decodeEntities(stripTags(t))}\``);
}

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
    if (rows.length >= 1) {
      const colCount = rows[0].split('|').length - 2;
      rows.splice(1, 0, '| ' + Array(colCount).fill('---').join(' | ') + ' |');
    }
    return '\n\n' + rows.join('\n') + '\n\n';
  });
}

function rewriteDivs(s) {
  return s
    .replace(/<\/(div|aside|section|header|footer|article|details|summary|span)[^>]*>/gi, '')
    .replace(/<(div|aside|section|header|footer|article|details|summary|span)[^>]*>/gi, '');
}

function rewriteCallouts(s) {
  // Mintlify supports <Note>, <Tip>, <Warning> components. Map .callout
  // div classes to those.
  return s
    .replace(/<div[^>]*class="callout note"[^>]*>([\s\S]*?)<\/div>/gi, (_, body) => `\n<Note>\n${stripTags(body).trim()}\n</Note>\n`)
    .replace(/<div[^>]*class="callout tip"[^>]*>([\s\S]*?)<\/div>/gi,  (_, body) => `\n<Tip>\n${stripTags(body).trim()}\n</Tip>\n`)
    .replace(/<div[^>]*class="callout warn"[^>]*>([\s\S]*?)<\/div>/gi, (_, body) => `\n<Warning>\n${stripTags(body).trim()}\n</Warning>\n`);
}

function tidy(s) {
  return decodeEntities(s)
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .replace(/^\s+|\s+$/g, '') + '\n';
}

function htmlToMarkdown(html) {
  let s = html;
  s = stripChrome(s);
  s = rewriteCallouts(s);    // before generic divs strip
  s = rewriteTables(s);
  s = rewriteCodeBlocks(s);
  s = rewriteCards(s);
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

// Mintlify pages use frontmatter for title + description; the H1 is
// typically dropped from the body to avoid duplication.
function buildPage(p, body) {
  // Drop the leading H1 (we put title in frontmatter instead).
  const stripped = body.replace(/^#\s+[^\n]+\n+/, '');
  const fm = ['---'];
  fm.push(`title: "${p.title}"`);
  if (p.desc) fm.push(`description: "${p.desc}"`);
  fm.push('---');
  return fm.join('\n') + '\n\n' + stripped;
}

// Rewrite intra-site links from old paths to the new docs slug paths
// where possible.
const PATH_REWRITES = [
  [/\/sdk\/python\/app\/?/g,           '/python/app'],
  [/\/sdk\/python\/event\/?/g,         '/python/event'],
  [/\/sdk\/python\/table\/?/g,         '/python/table'],
  [/\/sdk\/python\/col-lit\/?/g,       '/python/col-lit'],
  [/\/sdk\/python\/operators\/?/g,     '/python/operators'],
  [/\/sdk\/python\/errors\/?/g,        '/python/errors'],
  [/\/sdk\/python\/?/g,                '/quickstart'],
  [/\/sdk\/server\/?/g,                '/server-config'],
  [/\/sdk\/http\/push\/?/g,            '/http/push'],
  [/\/sdk\/http\/get\/?/g,             '/http/get'],
  [/\/sdk\/http\/register\/?/g,        '/http/register'],
  [/\/sdk\/http\/wire-spec\/?/g,       '/http/wire-spec'],
  [/\/docs\/concepts\/streams\/?/g,    '/concepts/streams'],
  [/\/docs\/concepts\/tables\/?/g,     '/concepts/tables'],
  [/\/docs\/concepts\/windows\/?/g,    '/concepts/windows'],
  [/\/docs\/concepts\/freshness\/?/g,  '/concepts/freshness'],
  [/\/docs\/concepts\/get-and-batch-get\/?/g, '/concepts/get-and-batch-get'],
];

function rewritePaths(s) {
  for (const [pat, repl] of PATH_REWRITES) s = s.replace(pat, repl);
  return s;
}

// ─── docs.json builder ─────────────────────────────────────────

function buildDocsJson(pages) {
  const groups = new Map();
  for (const p of pages) {
    if (!groups.has(p.section)) groups.set(p.section, []);
    groups.get(p.section).push(p.slug);
  }
  return {
    $schema: 'https://mintlify.com/docs.json',
    theme: 'mint',
    name: 'beava',
    description: 'Real-time feature server for fraud, ad-tech, and behavioral analytics. Single binary, in-memory state, declarative pipeline.',
    colors: { primary: '#b85c20', light: '#d97a3e', dark: '#a04e16' },
    background: { color: { light: '#fdfaf4', dark: '#1a1714' } },
    fonts: {
      heading: { family: 'Alegreya', weight: 600 },
      body: { family: 'Inter Tight', weight: 400 },
    },
    styling: { eyebrow: 'section' },
    navigation: {
      tabs: [{
        tab: 'Documentation',
        groups: [
          { group: 'Overview', pages: ['index'] },
          ...Array.from(groups.entries()).map(([group, pages]) => ({ group, pages })),
        ],
      }],
    },
    footer: { socials: { github: 'https://github.com/beava-dev/beava', discord: 'https://discord.gg/Jnx89PN9' } },
  };
}

// ─── main ─────────────────────────────────────────────────────

function main() {
  fs.mkdirSync(OUT_ROOT, { recursive: true });
  let written = 0;
  for (const p of PAGES) {
    const fp = path.join(SITE_ROOT, p.src);
    if (!fs.existsSync(fp)) {
      console.warn(`skip missing: ${fp}`);
      continue;
    }
    const html = fs.readFileSync(fp, 'utf8');
    const main = extractMain(html);
    if (!main) {
      console.warn(`skip no <main>: ${fp}`);
      continue;
    }
    const md = rewritePaths(htmlToMarkdown(main));
    const out = path.join(OUT_ROOT, `${p.slug}.mdx`);
    fs.mkdirSync(path.dirname(out), { recursive: true });
    fs.writeFileSync(out, buildPage(p, md));
    written++;
    console.log(`  ${path.relative(REPO_ROOT, out)}  (${md.split('\n').length} lines)`);
  }

  // docs.json
  const docsJson = buildDocsJson(PAGES);
  fs.writeFileSync(path.join(OUT_ROOT, 'docs.json'), JSON.stringify(docsJson, null, 2) + '\n');
  console.log(`\nwrote ${written} pages + docs.json`);
}

main();
