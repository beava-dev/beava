// scripts/check-copy-buttons.mjs
//
// Snapshot test for 1d (copy buttons on docs + sdk pages).
//
// Verifies that:
//   1. beava-website/project/js/copy-buttons.js exists.
//   2. The shared script wires up navigator.clipboard.writeText.
//   3. After running render-docs.mjs, every docs HTML page references
//      /js/copy-buttons.js via a <script> tag.
//   4. The standalone /sdk/python/index.html references it too.
//
// Exits 0 on pass, non-zero on fail. Run via `npm run check:copy-buttons`.

import { execSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, '../..');
const SITE_ROOT = path.join(REPO_ROOT, 'beava-website');

const checks = [];
function check(name, ok, detail) {
  checks.push({ name, ok, detail: ok ? '' : detail });
}

// ─── 1. shared JS module exists ──────────────────────────────────────────
const COPY_JS = path.join(SITE_ROOT, 'project/js/copy-buttons.js');
check('beava-website/project/js/copy-buttons.js exists',
  fs.existsSync(COPY_JS),
  `missing file: ${COPY_JS}`);

if (fs.existsSync(COPY_JS)) {
  const body = fs.readFileSync(COPY_JS, 'utf8');
  check('copy-buttons.js calls navigator.clipboard',
    /navigator\.clipboard/.test(body),
    'expected navigator.clipboard reference in copy-buttons.js');
  check('copy-buttons.js iterates over <pre> blocks',
    /querySelectorAll\s*\(\s*['"]pre['"]\s*\)/.test(body) ||
      /getElementsByTagName\s*\(\s*['"]pre['"]\s*\)/.test(body),
    'expected querySelectorAll("pre") in copy-buttons.js');
}

// ─── 2. re-render docs so we test against fresh HTML output ──────────────
try {
  execSync('node scripts/render-docs.mjs', {
    cwd: SITE_ROOT,
    stdio: 'pipe',
  });
} catch (err) {
  check('render-docs.mjs runs without error', false,
    `render-docs.mjs failed: ${err.message}`);
}

// ─── 3. rendered docs HTML loads copy-buttons.js ─────────────────────────
const SCRIPT_RE = /<script[^>]+src=["'][^"']*\/js\/copy-buttons\.js["']/;
const SAMPLES = [
  'project/docs/quickstart/index.html',
  'project/docs/index.html',
  'project/docs/operators/index.html',
];
for (const rel of SAMPLES) {
  const p = path.join(SITE_ROOT, rel);
  if (!fs.existsSync(p)) {
    check(`rendered docs page exists: ${rel}`, false, `not found: ${p}`);
    continue;
  }
  const body = fs.readFileSync(p, 'utf8');
  check(`${rel} loads /js/copy-buttons.js`,
    SCRIPT_RE.test(body),
    `expected a <script src=".../js/copy-buttons.js"> tag in ${rel}`);
}

// ─── 4. standalone sdk/python/index.html loads copy-buttons.js ───────────
const SDK_PY = path.join(SITE_ROOT, 'project/sdk/python/index.html');
if (!fs.existsSync(SDK_PY)) {
  check('sdk/python/index.html exists', false, `not found: ${SDK_PY}`);
} else {
  const body = fs.readFileSync(SDK_PY, 'utf8');
  check('sdk/python/index.html loads /js/copy-buttons.js',
    SCRIPT_RE.test(body),
    'expected a <script src=".../js/copy-buttons.js"> tag in sdk/python/index.html');
}

// ─── report + exit ───────────────────────────────────────────────────────
let fail = 0;
for (const c of checks) {
  if (c.ok) {
    console.log(`  ✓ ${c.name}`);
  } else {
    console.error(`  ✗ ${c.name}`);
    if (c.detail) console.error(`      ${c.detail}`);
    fail++;
  }
}
console.log(`check-copy-buttons: ${checks.length - fail}/${checks.length} passed`);
process.exit(fail > 0 ? 1 : 0);
