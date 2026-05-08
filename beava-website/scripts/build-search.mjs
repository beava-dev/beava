// scripts/build-search.mjs
//
// Phase 13.7 — Pagefind search index builder.
//
// The renderer (render-docs.mjs) emits static HTML with `data-pagefind-body`
// on the <main> element of each docs page. Pagefind's directory crawler
// indexes those automatically. Output lives at project/_pagefind/.
//
// We also index the legacy hand-rolled pages (project/index.html,
// project/field-guide-ch{1,2}.html, project/guide/**/index.html) via
// addCustomRecord because those pages render their content inside React+Babel
// templates that Pagefind's HTML parser cannot extract from.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import * as pagefind from 'pagefind';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '../..');
const SITE_ROOT = path.join(REPO_ROOT, 'beava-website/project');
const OUT_DIR = path.join(SITE_ROOT, '_pagefind');

// Curated entries for legacy React+Babel pages. Pagefind cannot extract
// content from <script type="text/babel"> templates so we feed plain text
// summaries here. URLs are absolute site paths.
const LEGACY_PAGES = [
  {
    url: '/',
    meta: { title: 'beava — dam good at streams', section: 'Home' },
    content: `beava is a single-binary real-time feature server for fraud, ad-tech, and behavioral analytics.
      Push events in over HTTP, declare aggregations, query computed features by entity key. Apache 2.0.
      Personalization, fraud rules, live dashboards in hours. Single binary. brew install beava.
      curl install. docker run beava. Read the guide. Star on GitHub.`,
  },
  {
    url: '/guide/',
    meta: { title: 'The beava guidebook', section: 'Guide' },
    content: `The real-time feature guidebook. How to build the live features your product actually
      needs. Chapter 1 is a 10-minute interactive build that turns into a per-customer analytics
      dashboard. Pick the recipe that matches your day job: fraud, personalization, ranking, rate
      limits, metering.`,
  },
  {
    url: '/guide/chapter-1/',
    meta: { title: 'Guidebook chapter 1', section: 'Guide' },
    content: `Chapter 1: from zero to a per-customer analytics dashboard. A 10-minute interactive
      build using beava's stream and table operators. Pedagogy first; reference second.`,
  },
  // /guide/recipes/fraud/ existed at the time this list was authored
  // but was deleted in 0d285744 ("drop broken internal links to
  // deleted rendered docs"). Removed from the index list 2026-05-08.
  {
    url: '/field-guide-ch1.html',
    meta: { title: 'Field guide chapter 1', section: 'Field guide' },
    content: `Field guide chapter 1: introducing beava, streams vs tables, and the first
      pipeline.`,
  },
  {
    url: '/field-guide-ch2.html',
    meta: { title: 'Field guide chapter 2', section: 'Field guide' },
    content: `Field guide chapter 2: aggregation operators, windows, and per-entity feature
      serving.`,
  },
  {
    url: '/design-system/',
    meta: { title: 'Design system', section: 'Design' },
    content: `beava design system: colors, type, components. Burnt orange accent, cream surface,
      Alegreya serif headings, Inter Tight UI sans, JetBrains Mono code.`,
  },
];

async function main() {
  // Clear previous output for a clean build
  if (fs.existsSync(OUT_DIR)) {
    fs.rmSync(OUT_DIR, { recursive: true, force: true });
  }

  const { index } = await pagefind.createIndex({
    rootSelector: 'html',
    excludeSelectors: ['.docs-sidebar', '.docs-toc', '.site-nav', '.site-foot', 'nav.crumbs'],
    forceLanguage: 'en',
  });

  // Index the rendered docs tree (static HTML with data-pagefind-body)
  //
  // KNOWN REGRESSION (2026-05-08): docs/**/*.html pages are React+Babel
  // templates that use JSX `className` instead of HTML `class`, so
  // Pagefind's HTML crawler can extract NOTHING from them. addDirectory
  // reports the file count but page_count in the final index drops to
  // ~0 from these pages. Net effect: docs/ is currently un-indexable.
  //
  // To fix: either (a) curated addCustomRecord entries per docs page
  // matching the LEGACY_PAGES pattern below, or (b) introduce an SSR
  // build step that renders the docs/* templates to real static HTML
  // before this script runs. Tracked separately; not blocking SDK
  // pages, which use real `class` attributes and ARE indexed correctly.
  const dirRes = await index.addDirectory({
    path: SITE_ROOT,
    glob: 'docs/**/*.html',
  });
  console.log(`addDirectory: ${dirRes.page_count} pages from project/docs/`);

  // Index the SDK reference pages. These are hand-written HTML (not
  // markdown-rendered) but the prose lives in plain elements inside
  // `<main class="content" data-pagefind-body>`, so Pagefind's HTML
  // parser picks them up directly. Sidebar / TOC / pager are mounted
  // via React+Babel into divs OUTSIDE the data-pagefind-body main, so
  // they're excluded from the index without further configuration.
  const sdkRes = await index.addDirectory({
    path: SITE_ROOT,
    glob: 'sdk/**/*.html',
  });
  console.log(`addDirectory: ${sdkRes.page_count} pages from project/sdk/`);

  // Curated legacy entries
  for (const p of LEGACY_PAGES) {
    await index.addCustomRecord({
      url: p.url,
      content: p.content,
      language: 'en',
      meta: p.meta,
    });
  }
  console.log(`addCustomRecord: ${LEGACY_PAGES.length} legacy pages`);

  const writeRes = await index.writeFiles({ outputPath: OUT_DIR });
  console.log(`writeFiles: ${writeRes.outputPath}`);

  await pagefind.close();
  console.log('build-search: done');
}

main().catch((err) => {
  console.error('build-search failed:', err);
  process.exit(1);
});
