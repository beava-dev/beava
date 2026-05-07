// Page-view tracker.
//
// On load: timer starts. On hide/unload: POSTs {path, dwell_ms} to
// /api/push/PageView. Pure HTTP — no SDK shim, no client-side
// aggregation, no bucket sentinel. The beava-website-beava instance has
// a registered PageView event + SiteMetrics derivation (keyless table
// per ADR-003); every event flows into the global SiteMetrics row
// server-side. Anyone running `pip install beava` + `beava` would see
// the same wire format.
(function () {
  var startedAt = performance.now();
  var path = location.pathname || '/';
  var sent = false;

  function send() {
    if (sent) return;
    sent = true;
    var body = JSON.stringify({ path: path, dwell_ms: Math.round(performance.now() - startedAt) });
    try {
      if (navigator.sendBeacon) {
        navigator.sendBeacon('/api/push/PageView', new Blob([body], { type: 'application/json' }));
      } else {
        fetch('/api/push/PageView', {
          method: 'POST',
          body: body,
          headers: { 'Content-Type': 'application/json' },
          keepalive: true,
        });
      }
    } catch (_) { /* drop on the floor; tracker must never block UX */ }
  }

  window.addEventListener('pagehide', send);
  window.addEventListener('beforeunload', send);
  document.addEventListener('visibilitychange', function () {
    if (document.visibilityState === 'hidden') send();
  });
})();
