// copy-buttons.js — adds a "copy" button to every <pre> block on the page.
//
// Loaded via <script src="/js/copy-buttons.js" defer> from rendered docs
// pages (beava-website/scripts/render-docs.mjs) and from the standalone
// /sdk/python/index.html. Vanilla JS (no React) so it works inside the
// hand-rolled static HTML the docs site ships.
//
// Behaviour:
//   - On DOMContentLoaded (or immediately if the document is already
//     interactive), scan for every <pre> block.
//   - Wrap each <pre> in a `position: relative` <div class="codeblock-wrap">
//     and inject a "copy" button in the top-right corner.
//   - Clicking the button copies the <pre>'s textContent via
//     navigator.clipboard.writeText, flips the label to "copied" for
//     ~1.4s, then restores it.
//   - Skips empty <pre> blocks and ones already wrapped (so re-running
//     the init function is idempotent — useful in case some future page
//     loads content asynchronously).
//
// Graceful degradation: if JS is disabled, the user sees the unmodified
// <pre> exactly as the page would render without this script. They can
// still select + copy by hand.

(function () {
  'use strict';

  function injectStyles() {
    if (document.getElementById('copy-button-styles')) return;
    var style = document.createElement('style');
    style.id = 'copy-button-styles';
    style.textContent = [
      '.codeblock-wrap { position: relative; }',
      '.codeblock-wrap .copy-btn {',
      '  position: absolute;',
      '  top: 8px;',
      '  right: 8px;',
      '  background: #fff;',
      '  border: 1px solid var(--border, #d4cfc4);',
      '  color: var(--fg3, #6b655c);',
      '  border-radius: 6px;',
      '  padding: 4px 10px;',
      '  font-size: 11px;',
      '  font-family: var(--font-sans, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif);',
      '  font-weight: 600;',
      '  letter-spacing: 0.02em;',
      '  cursor: pointer;',
      '  opacity: 0;',
      '  transition: opacity 160ms ease, color 160ms ease, border-color 160ms ease;',
      '  z-index: 2;',
      '  user-select: none;',
      '}',
      '.codeblock-wrap:hover .copy-btn,',
      '.codeblock-wrap:focus-within .copy-btn,',
      '.codeblock-wrap .copy-btn:focus { opacity: 1; }',
      '.codeblock-wrap .copy-btn.copied {',
      '  color: var(--beava-success, #2f7d3e);',
      '  border-color: var(--beava-success, #2f7d3e);',
      '  opacity: 1;',
      '}',
      '@media (max-width: 640px) {',
      '  .codeblock-wrap .copy-btn { opacity: 1; }',
      '}',
    ].join('\n');
    document.head.appendChild(style);
  }

  function makeButton(getText) {
    var btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'copy-btn';
    btn.setAttribute('aria-label', 'Copy code to clipboard');
    btn.textContent = 'copy';
    btn.addEventListener('click', function () {
      var text = getText();
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(flash, function () {
          fallback(text);
          flash();
        });
      } else {
        fallback(text);
        flash();
      }
    });
    function flash() {
      btn.textContent = 'copied';
      btn.classList.add('copied');
      setTimeout(function () {
        btn.textContent = 'copy';
        btn.classList.remove('copied');
      }, 1400);
    }
    return btn;
  }

  // Best-effort fallback for ancient browsers / non-secure contexts where
  // navigator.clipboard is unavailable. Uses an off-screen <textarea> +
  // document.execCommand('copy'). Silently no-ops if even that fails.
  function fallback(text) {
    var ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'absolute';
    ta.style.left = '-9999px';
    document.body.appendChild(ta);
    ta.select();
    try {
      document.execCommand('copy');
    } catch (_) {
      /* swallow — best-effort */
    }
    document.body.removeChild(ta);
  }

  // Wire an existing `<button class="copy-btn">` to copy `pre`'s
  // textContent. Used when a page already ships its own button markup
  // (e.g. /sdk/python/index.html's `.codeblock-head` pattern).
  function wireExisting(btn, pre) {
    if (btn.dataset.copyWired === '1') return;
    btn.dataset.copyWired = '1';
    var origText = btn.textContent;
    btn.addEventListener('click', function () {
      var text = pre.textContent || '';
      if (navigator.clipboard && navigator.clipboard.writeText) {
        navigator.clipboard.writeText(text).then(flash, function () {
          fallback(text);
          flash();
        });
      } else {
        fallback(text);
        flash();
      }
      function flash() {
        btn.textContent = '✓ copied';
        btn.classList.add('copied');
        setTimeout(function () {
          btn.textContent = origText;
          btn.classList.remove('copied');
        }, 1400);
      }
    });
  }

  function init() {
    injectStyles();
    var pres = document.querySelectorAll('pre');
    pres.forEach(function (pre) {
      // Skip empty blocks — no point copying nothing.
      if (!pre.textContent || !pre.textContent.trim()) return;

      // Case 1: a sibling `.copy-btn` already exists in the same
      // `.codeblock` container (sdk/python/index.html's
      // `.codeblock-head` pattern). Wire it up to copy `pre` and
      // skip injection.
      var container = pre.closest('.codeblock');
      if (container) {
        var existing = container.querySelector('.copy-btn');
        if (existing) {
          wireExisting(existing, pre);
          return;
        }
      }

      // Case 2: skip if already wrapped by a previous init() pass.
      if (
        pre.parentElement &&
        pre.parentElement.classList &&
        pre.parentElement.classList.contains('codeblock-wrap')
      ) {
        return;
      }

      // Case 3: no pre-existing button — inject a corner-floating one.
      var wrap = document.createElement('div');
      wrap.className = 'codeblock-wrap';
      pre.parentNode.insertBefore(wrap, pre);
      wrap.appendChild(pre);
      // forEach gives each iteration its own `pre` binding, so the
      // closure below captures the right block.
      var btn = makeButton(function () {
        return pre.textContent || '';
      });
      wrap.appendChild(btn);
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', init);
  } else {
    init();
  }
})();
