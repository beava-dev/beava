// scripts/build-mintlify-docs.mjs
//
// Converts the SDK reference + concepts pages from
// beava-website/project/sdk/ and beava-website/project/docs/concepts/
// into Mintlify-shaped MDX files under repo-root /docs/.
//
// Mintlify-aware transforms — preserves the website's text verbatim but
// rewrites structural elements (codeblock-head with filename + Copy
// button, parameter rows, tabs, accordions, method signatures) into
// Mintlify components so the generated MDX renders cleanly without
// label leaks.

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
  let m = html.match(/<main[^>]*class="content"[^>]*>([\s\S]*?)<\/main>/);
  if (m) return m[1];
  m = html.match(/<main[^>]*className="bv-content"[^>]*>([\s\S]*?)<\/main>/);
  return m ? m[1] : null;
}

function stripChrome(s) {
  return s
    .replace(/<!--[\s\S]*?-->/g, '')
    .replace(/<script[\s\S]*?<\/script>/gi, '')
    .replace(/<style[\s\S]*?<\/style>/gi, '')
    .replace(/<svg[\s\S]*?<\/svg>/gi, '')
    .replace(/<button[^>]*class="copy-btn"[^>]*>[\s\S]*?<\/button>/gi, '')
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

// Heuristic: filename or label → fenced-code language.
function langFromFilename(filename) {
  if (!filename) return '';
  const lc = filename.toLowerCase();
  if (lc.endsWith('.py'))                            return 'python';
  if (lc.endsWith('.json'))                          return 'json';
  if (lc.endsWith('.toml'))                          return 'toml';
  if (lc.endsWith('.yaml') || lc.endsWith('.yml'))   return 'yaml';
  if (lc.endsWith('.sh'))                            return 'bash';
  if (lc === 'terminal' || lc === 'shell')           return 'bash';
  // beava convention: bodies named "request" / "response" / "*body" /
  // "cold-start *" are JSON wire payloads.
  if (/(?:^|[^\w])(request|response|body|payload)(?:$|[^\w])/i.test(filename)
      && !lc.endsWith('.py')) return 'json';
  // "signature" labels code blocks holding type signatures — leave
  // unset so Mintlify renders it as plain monospace without
  // syntax-highlighting attempts.
  return '';
}

// `<div class="codeblock">` (with codeblock-head label) and bare `<pre>`
// — the former leaks "signature"/"register.py"/"Copy" without explicit
// handling; the latter is fine but loses language-hinting. Both folded
// into Mintlify fenced code with optional `title="…"` from the
// filename label.
function rewriteCodeBlocks(s) {
  // First the wrapped form (with codeblock-head).
  s = s.replace(
    /<div[^>]*class="codeblock"[^>]*>\s*<div[^>]*class="codeblock-head"[^>]*>([\s\S]*?)<\/div>\s*<pre[^>]*>([\s\S]*?)<\/pre>\s*<\/div>/gi,
    (_, head, body) => {
      const filename = (head.match(/<span[^>]*class="filename"[^>]*>([\s\S]*?)<\/span>/i) || [, ''])[1].trim();
      const code = decodeEntities(stripTags(body)).replace(/\n+$/, '');
      const lang  = langFromFilename(filename);
      const title = filename ? ` title="${filename}"` : '';
      return `\n\n\`\`\`${lang}${title}\n${code}\n\`\`\`\n`;
    }
  );
  // codeblock wrapper with NO head (the rare unlabeled variant).
  s = s.replace(
    /<div[^>]*class="codeblock"[^>]*>\s*<pre[^>]*>([\s\S]*?)<\/pre>\s*<\/div>/gi,
    (_, body) => {
      const code = decodeEntities(stripTags(body)).replace(/\n+$/, '');
      return `\n\n\`\`\`\n${code}\n\`\`\`\n`;
    }
  );
  // Bare <pre> (concept pages, RFCs).
  s = s.replace(/<pre[^>]*>([\s\S]*?)<\/pre>/gi, (_, body) => {
    const code = decodeEntities(stripTags(body)).replace(/\n+$/, '');
    return `\n\n\`\`\`\n${code}\n\`\`\`\n`;
  });
  return s;
}

// Find each `<div class="param">…</div>` and rewrite to a Mintlify
// <ParamField>. Operates independently of the (sometimes-present) outer
// `<div class="params">` wrapper. The block's outer `</div>` is found by
// nesting-aware scanning so nested `<div class="param-head">` /
// `<div class="param-body">` close their own divs without prematurely
// terminating the outer match.
function rewriteParams(s) {
  const out = [];
  let i = 0;
  const open = /<div[^>]*class="param"[^>]*>/gi;
  while (i < s.length) {
    open.lastIndex = i;
    const m = open.exec(s);
    if (!m) { out.push(s.slice(i)); break; }
    out.push(s.slice(i, m.index));
    const startBody = m.index + m[0].length;
    // Walk div nesting from depth 1; consume until matched </div>.
    const block = consumeBalancedDiv(s, startBody);
    if (block.end === -1) {
      // Unbalanced — bail out, leave the block as-is.
      out.push(s.slice(m.index, m.index + m[0].length));
      i = m.index + m[0].length;
      continue;
    }
    out.push(renderParamField(s.slice(startBody, block.end)));
    i = block.endAfter;
  }
  return out.join('');
}

// Given `s` and an offset just after a `<div ...>` opening tag, find
// the matching `</div>` honoring nested div opens/closes. Returns
// `{ end, endAfter }` or `{ end: -1 }` if unbalanced.
function consumeBalancedDiv(s, from) {
  const tagRe = /<\/?div\b[^>]*>/gi;
  tagRe.lastIndex = from;
  let depth = 1;
  let m;
  while ((m = tagRe.exec(s)) !== null) {
    if (m[0][1] === '/') {
      depth--;
      if (depth === 0) return { end: m.index, endAfter: tagRe.lastIndex };
    } else {
      depth++;
    }
  }
  return { end: -1, endAfter: -1 };
}

function renderParamField(inner) {
  const name = (inner.match(/<span[^>]*class="param-name"[^>]*>([\s\S]*?)<\/span>/i) || [, ''])[1].trim();
  const type = (inner.match(/<span[^>]*class="param-tag(?:\s+\w+)?"[^>]*>([\s\S]*?)<\/span>/i) || [, ''])[1].trim();
  const required = /class="param-tag[^"]*\brequired\b/i.test(inner);
  const bodyMatch = inner.match(/<div[^>]*class="param-body"[^>]*>([\s\S]*?)<\/div>\s*$/i)
                 || inner.match(/<div[^>]*class="param-body"[^>]*>([\s\S]*?)<\/div>/i);
  let body = bodyMatch ? bodyMatch[1] : '';
  let defaultVal = '';
  body = body.replace(/<p[^>]*class="default"[^>]*>([\s\S]*?)<\/p>/gi, (_, d) => {
    const m2 = d.match(/<code[^>]*>([\s\S]*?)<\/code>/);
    defaultVal = (m2 ? m2[1] : stripTags(d).replace(/^default\s+/i, '')).trim();
    return '';
  });
  // Body: keep inline code as backticks, collapse <p> to paragraph breaks.
  let bodyMd = body
    .replace(/<code[^>]*>([\s\S]*?)<\/code>/gi, (_, t) => `\`${stripTags(t)}\``)
    .replace(/<p[^>]*>([\s\S]*?)<\/p>/gi, (_, t) => t.trim() + '\n\n');
  bodyMd = decodeEntities(stripTags(bodyMd)).trim();
  const attrs = [`path="${escAttr(name)}"`];
  if (type) attrs.push(`type="${escAttr(type)}"`);
  if (required) attrs.push('required');
  if (defaultVal) attrs.push(`default="${escAttr(defaultVal)}"`);
  return `\n\n<ParamField ${attrs.join(' ')}>\n${bodyMd}\n</ParamField>\n`;
}

function escAttr(s) {
  return String(s).replace(/"/g, '\\"');
}

// `<div class="method-head"> <h3>NAME</h3> <div class="sig">SIG</div> </div>`
// → `### NAME` + python-fenced signature.
function rewriteMethodHead(s) {
  return s.replace(/<div[^>]*class="method-head"[^>]*>([\s\S]*?)<\/div>/gi, (_, body) => {
    const h3  = (body.match(/<h3[^>]*>([\s\S]*?)<\/h3>/i)               || [, ''])[1];
    const sig = (body.match(/<div[^>]*class="sig"[^>]*>([\s\S]*?)<\/div>/i) || [, ''])[1];
    const headText = decodeEntities(stripTags(h3)).trim();
    if (!sig) return `\n\n### ${headText}\n`;
    const sigText = decodeEntities(stripTags(sig)).trim();
    return `\n\n### ${headText}\n\n\`\`\`python\n${sigText}\n\`\`\`\n`;
  });
}

// <details><summary>Q</summary>…</details>, possibly wrapped in
// <div class="accordion">. Outputs Mintlify <Accordion> /
// <AccordionGroup>.
function rewriteAccordions(s) {
  // First wrap groups: <div class="accordion">multiple <details>...</div>
  s = s.replace(/<div[^>]*class="accordion"[^>]*>([\s\S]*?)<\/div>(?=\s*(?:<h[1-6]|<\/main>|<div[^>]*class="(?!accordion)|<p|$))/gi,
    (_, body) => {
      const items = [];
      body.replace(/<details[^>]*>([\s\S]*?)<\/details>/gi, (_, inner) => {
        const sum = (inner.match(/<summary[^>]*>([\s\S]*?)<\/summary>/i) || [, ''])[1];
        const rest = inner.replace(/<summary[^>]*>[\s\S]*?<\/summary>/i, '');
        items.push({ title: stripTags(sum).trim(), body: rest });
        return '';
      });
      if (items.length === 0) return body;
      const inner = items.map(i =>
        `<Accordion title="${escAttr(decodeEntities(i.title))}">\n${i.body.trim()}\n</Accordion>`
      ).join('\n\n');
      return `\n\n<AccordionGroup>\n${inner}\n</AccordionGroup>\n`;
    });
  // Loose single details (not inside an accordion wrapper).
  s = s.replace(/<details[^>]*>([\s\S]*?)<\/details>/gi, (_, inner) => {
    const sum = (inner.match(/<summary[^>]*>([\s\S]*?)<\/summary>/i) || [, ''])[1];
    const rest = inner.replace(/<summary[^>]*>[\s\S]*?<\/summary>/i, '');
    return `\n\n<Accordion title="${escAttr(decodeEntities(stripTags(sum).trim()))}">\n${rest.trim()}\n</Accordion>\n`;
  });
  return s;
}

// `<div class="tabs"> <div class="tabs-strip"><button data-target="t-X">L</button>…</div>
//  <div class="tabs-panel" id="t-X">…</div>… </div>`
// → Mintlify <Tabs><Tab title="L">…</Tab>…</Tabs>.
function rewriteTabs(s) {
  return s.replace(/<div[^>]*class="tabs"[^>]*>([\s\S]*?)<\/div>(?=\s*(?:<h[1-6]|<\/main>|<p|<div|<details))/gi,
    (_, body) => {
      // Build (id → label) map from tabs-strip.
      const stripMatch = body.match(/<div[^>]*class="tabs-strip"[^>]*>([\s\S]*?)<\/div>/i);
      if (!stripMatch) return body;
      const strip = stripMatch[1];
      const labels = new Map();
      const btnRe = /<button[^>]*data-target="([^"]+)"[^>]*>([\s\S]*?)<\/button>/gi;
      let bm;
      while ((bm = btnRe.exec(strip)) !== null) {
        labels.set(bm[1], decodeEntities(stripTags(bm[2]).trim()));
      }
      // Walk panels.
      const panelRe = /<div[^>]*class="tabs-panel(?:\s+active)?"[^>]*id="([^"]+)"[^>]*>([\s\S]*?)<\/div>(?=\s*(?:<div[^>]*class="tabs-panel"|<\/div>))/gi;
      const tabs = [];
      let pm;
      while ((pm = panelRe.exec(body)) !== null) {
        const id = pm[1];
        const label = labels.get(id) || id;
        tabs.push({ label, content: pm[2] });
      }
      if (tabs.length === 0) return body;
      const rendered = tabs.map(t => `<Tab title="${escAttr(t.label)}">\n${t.content.trim()}\n</Tab>`).join('\n');
      return `\n\n<Tabs>\n${rendered}\n</Tabs>\n`;
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
    .replace(/<h1\b[^>]*>([\s\S]*?)<\/h1>/gi, (_, t) => `\n\n# ${stripTags(t).trim()}\n`)
    .replace(/<h2\b[^>]*>([\s\S]*?)<\/h2>/gi, (_, t) => `\n\n## ${stripTags(t).trim()}\n`)
    .replace(/<h3\b[^>]*>([\s\S]*?)<\/h3>/gi, (_, t) => `\n\n### ${stripTags(t).trim()}\n`)
    .replace(/<h4\b[^>]*>([\s\S]*?)<\/h4>/gi, (_, t) => `\n\n#### ${stripTags(t).trim()}\n`);
}

function rewriteLists(s) {
  return s
    .replace(/<li\b[^>]*>([\s\S]*?)<\/li>/gi, (_, t) => `\n- ${stripTags(t).trim()}`)
    .replace(/<\/?(ul|ol)\b[^>]*>/gi, '\n');
}

function rewriteEmphasis(s) {
  // `<b\b...>...</b>` and `<strong\b...>...</strong>`. The `\b` matters
  // because `<b ...>` is a prefix of `<button>`, `<body>`, etc. — a
  // bare `<b[^>]*>` would consume any tag starting with `<b`.
  return s
    .replace(/<strong\b[^>]*>([\s\S]*?)<\/strong>/gi, (_, body) => `**${stripTags(body)}**`)
    .replace(/<b\b[^>]*>([\s\S]*?)<\/b>/gi, (_, body) => `**${stripTags(body)}**`)
    .replace(/<em\b[^>]*>([\s\S]*?)<\/em>/gi, (_, body) => `*${stripTags(body)}*`)
    .replace(/<i\b[^>]*>([\s\S]*?)<\/i>/gi, (_, body) => `*${stripTags(body)}*`);
}

function rewriteParagraphs(s) {
  // `<p\b...>` to avoid swallowing `<ParamField>` (which starts with
  // `<Pa…` — ambiguous with `<p…>` under a non-anchored regex).
  return s.replace(/<p\b[^>]*>/gi, '\n\n').replace(/<\/p>/gi, '').replace(/<br\s*\/?>/gi, '\n');
}

function rewriteInlineCode(s) {
  return s.replace(/<code\b[^>]*>([\s\S]*?)<\/code>/gi, (_, t) => `\`${decodeEntities(stripTags(t))}\``);
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
  return s
    .replace(/<div[^>]*class="callout note"[^>]*>([\s\S]*?)<\/div>/gi, (_, body) => `\n<Note>\n${decodeEntities(stripTags(body)).trim()}\n</Note>\n`)
    .replace(/<div[^>]*class="callout tip"[^>]*>([\s\S]*?)<\/div>/gi,  (_, body) => `\n<Tip>\n${decodeEntities(stripTags(body)).trim()}\n</Tip>\n`)
    .replace(/<div[^>]*class="callout warn"[^>]*>([\s\S]*?)<\/div>/gi, (_, body) => `\n<Warning>\n${decodeEntities(stripTags(body)).trim()}\n</Warning>\n`);
}

function tidy(s) {
  return decodeEntities(s)
    .replace(/[ \t]+\n/g, '\n')
    .replace(/\n{3,}/g, '\n\n')
    .replace(/^\s+|\s+$/g, '') + '\n';
}

// Strip remaining HTML tags but preserve Mintlify components
// (PascalCase: Tabs, Tab, ParamField, Note, Tip, Warning, Accordion,
// AccordionGroup, etc.) that the rewrite passes already emitted.
function stripNonComponentTags(s) {
  return s.replace(/<\/?([a-zA-Z][a-zA-Z0-9-]*)\b[^>]*>/g, (m, name) => {
    const ch = name[0];
    if (ch >= 'A' && ch <= 'Z') return m;
    return '';
  });
}

function htmlToMarkdown(html) {
  let s = html;
  s = stripChrome(s);
  // Structured Mintlify components — apply BEFORE generic <div> stripping
  // so we don't lose the surrounding wrapper context. Order matters:
  //   - rewriteParams must precede rewriteParagraphs (param bodies hold <p>)
  //   - rewriteCodeBlocks must precede rewriteInlineCode (avoid double-wrap)
  //   - rewriteTabs / rewriteAccordions must precede rewriteDivs
  s = rewriteCallouts(s);
  s = rewriteCodeBlocks(s);
  s = rewriteParams(s);
  s = rewriteMethodHead(s);
  s = rewriteAccordions(s);
  s = rewriteTabs(s);
  s = rewriteTables(s);
  s = rewriteCards(s);
  s = rewriteLinks(s);
  s = rewriteHeadings(s);
  s = rewriteLists(s);
  s = rewriteEmphasis(s);
  s = rewriteParagraphs(s);
  s = rewriteInlineCode(s);
  s = rewriteDivs(s);
  s = stripNonComponentTags(s);
  return tidy(s);
}

// Mintlify pages use frontmatter for title + description; the H1 is
// typically dropped from the body to avoid duplication.
function buildPage(p, body) {
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
    // Brand mark — same image the marketing site's SiteHeader/SiteFooter
    // use so users moving between beava.dev (HTML) and beava.dev/docs/
    // (Mintlify) see one identity.
    logo: { light: '/logo/beava.png', dark: '/logo/beava.png' },
    favicon: '/logo/beava.png',
    // Color tokens lifted from beava-design-system/project/colors_and_type.css
    // (--beava-orange / --beava-orange-soft / --beava-orange-dark for the
    // accent triad; --beava-cream / --beava-brown-ink for surfaces).
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
    // Top-of-page links — mirror the marketing-site SiteHeader nav so a
    // user reading docs sees the same site-level affordances. The "Docs"
    // self-link is omitted (we are docs); "SDK reference" maps to the
    // quickstart slug since the SDK pages now live under /python/* in
    // Mintlify (see PATH_REWRITES). GitHub becomes the navbar's primary
    // CTA per Mintlify convention.
    navbar: {
      links: [
        { label: 'Home', href: 'https://beava.dev/' },
        { label: 'Guide', href: 'https://beava.dev/guide/' },
        { label: 'Community', href: 'https://beava.dev/community/' },
        { label: 'Discord', href: 'https://discord.gg/Jnx89PN9' },
      ],
      primary: { type: 'github', href: 'https://github.com/beava-dev/beava' },
    },
    // Footer columns mirror beava-website's SiteFooter (Project +
    // Community groups), plus the same socials. Apache 2.0 / built-by
    // tagline lives in the project description above; Mintlify shows it
    // automatically alongside the logo.
    footer: {
      socials: {
        github: 'https://github.com/beava-dev/beava',
        discord: 'https://discord.gg/Jnx89PN9',
      },
      links: [
        {
          header: 'Project',
          items: [
            { label: 'Guide',          href: 'https://beava.dev/guide/' },
            { label: 'Docs',           href: '/' },
            { label: 'Roadmap',        href: 'https://github.com/beava-dev/beava/discussions' },
            { label: 'OSS commitment', href: 'https://github.com/beava-dev/beava/blob/main/LICENSE' },
          ],
        },
        {
          header: 'Community',
          items: [
            { label: 'GitHub',      href: 'https://github.com/beava-dev/beava' },
            { label: 'Discussions', href: 'https://github.com/beava-dev/beava/discussions' },
            { label: 'Discord',     href: 'https://discord.gg/Jnx89PN9' },
          ],
        },
      ],
    },
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

  const docsJson = buildDocsJson(PAGES);
  fs.writeFileSync(path.join(OUT_ROOT, 'docs.json'), JSON.stringify(docsJson, null, 2) + '\n');
  console.log(`\nwrote ${written} pages + docs.json`);
}

main();
