// docs-headings.js — copy-link icon on every H2/H3 with an id.
// Targets three surfaces:
//   .docs-prose  → markdown-rendered docs (mass-gen)
//   .bv-content  → React-based docs (vision, get-started, concepts, RFCs)
//   .content     → SDK reference pages under /sdk/ (App, @bv.event,
//                  HTTP API, etc.) — extended 2026-05-08 sub-session A
// Click copies the canonical URL with anchor to the clipboard and updates
// the address bar without scrolling.
(function () {
  function inject() {
    var selectors = [
      '.docs-prose h2[id]', '.docs-prose h3[id]',
      '.bv-content h2[id]', '.bv-content h3[id]',
      '.content h2[id]',    '.content h3[id]',
    ].join(', ');
    var headings = document.querySelectorAll(selectors);
    headings.forEach(function (h) {
      // Skip if already wired (idempotent across pjax-style renavigations)
      if (h.querySelector('.heading-anchor-link')) return;

      // Strip a leading "#" that markdown-it-anchor's linkInsideHeader leaves
      // behind in older builds, so we render cleanly.
      var firstAnchor = h.querySelector('a.header-anchor');
      if (firstAnchor) firstAnchor.remove();

      var a = document.createElement('a');
      a.className = 'heading-anchor-link';
      a.href = '#' + h.id;
      a.setAttribute('aria-label', 'Copy link to this section');
      a.title = 'Copy link';
      a.innerHTML = '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" style="width:14px;height:14px;display:block"><path d="M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-1 1"/><path d="M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l1-1"/></svg>';
      a.addEventListener('click', function (e) {
        e.preventDefault();
        var url = window.location.origin + window.location.pathname + '#' + h.id;
        try {
          navigator.clipboard && navigator.clipboard.writeText(url);
        } catch (_) {}
        a.classList.add('copied');
        setTimeout(function () { a.classList.remove('copied'); }, 1200);
        history.replaceState(null, '', '#' + h.id);
      });
      h.appendChild(a);
    });
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', inject);
  } else {
    inject();
  }

  // React-based pages render content after Babel finishes (which can be
  // arbitrary ms post-DOMContentLoaded). Watch the body for new H2/H3 nodes
  // and re-run inject() whenever the DOM changes. The inject() function
  // is idempotent (skips already-wired headings), so this is safe.
  var observer = new MutationObserver(function (muts) {
    for (var i = 0; i < muts.length; i++) {
      if (muts[i].addedNodes && muts[i].addedNodes.length > 0) {
        inject();
        return;
      }
    }
  });
  function start() {
    if (document.body) {
      observer.observe(document.body, { childList: true, subtree: true });
    } else {
      setTimeout(start, 100);
    }
  }
  start();
  // Stop observing after 8s to avoid permanent overhead on long-lived pages.
  setTimeout(function () { observer.disconnect(); }, 8000);
})();
