// Page-view tracker.
//
// On load: timer starts. On hide/unload: POSTs {session_id, path, dwell_ms}
// to /api/push/PageView. Pure HTTP — no SDK shim, no client-side
// aggregation, no bucket sentinel. The beava-website-beava instance has
// a registered PageView event + SiteMetrics derivation (keyless table
// per ADR-003); every event flows into the global SiteMetrics row
// server-side. Anyone running `pip install beava` + `beava` would see
// the same wire format. Schema is defined by
// beava-website/deploy/register_pipeline.py.
(function () {
  var startedAt = performance.now();
  var path = location.pathname || '/';
  var sent = false;

  // Anonymous per-tab session id. sessionStorage is cleared on tab close,
  // so this never persists across visits — matches the "Anonymous session
  // id per visit" claim on the homepage. No cookies, no fingerprinting.
  var sid;
  try {
    sid = sessionStorage.getItem('bv_sid');
    if (!sid) {
      sid = (crypto && crypto.randomUUID) ? crypto.randomUUID()
        : String(Date.now()) + '-' + Math.random().toString(36).slice(2);
      sessionStorage.setItem('bv_sid', sid);
    }
  } catch (_) {
    // sessionStorage unavailable (private mode, sandboxed iframe). Fall
    // back to an in-memory id; still valid for this page load.
    sid = String(Date.now()) + '-' + Math.random().toString(36).slice(2);
  }

  function send() {
    if (sent) return;
    sent = true;
    var body = JSON.stringify({
      session_id: sid,
      path: path,
      dwell_ms: Math.round(performance.now() - startedAt),
    });
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
