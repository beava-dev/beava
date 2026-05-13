// js/docs/DocsSidebar.jsx
// IA mirrors scripts/render-docs-config.json — keep both in sync.
// Top groups (Getting started, Concepts) start expanded.
// Reference / Architecture / Vision / Community start collapsed
// but auto-expand if the active page lives inside.

const Chev = () => (
  <svg className="bv-side-chev" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
    <path d="M9 6l6 6-6 6"/>
  </svg>
);

// IA: at most 3 entries per section. Sub-pages are kept on disk + indexed
// for search; just removed from sidebar nav. Re-surface by adding back here.
//
// Hidden-but-on-disk:
//   Getting started — /docs/get-started/query-features/
//   Concepts        — /docs/concepts/get-and-batch-get/, /docs/concepts/windows/,
//                     /docs/concepts/freshness/
//   Vision          — (none currently on disk)
//
// Reference / Architecture / Community sub-trees from the prior render-docs
// site were nuked in commit f99a09e1; re-add a section here when their
// hand-written replacements land.
const SECTIONS = [
  { title: 'Getting started', open: true, items: [
    { label: 'Introduction',         href: '/docs/' },
    { label: 'Quickstart',           href: '/docs/get-started/quickstart/' },
    { label: 'Build a pipeline',     href: '/docs/get-started/define-a-pipeline/' },
  ]},
  { title: 'Vision', open: true, items: [
    { label: 'Why beava',                href: '/docs/vision/why-beava/' },
    { label: 'Non-goals and tradeoffs',  href: '/docs/vision/non-goals/' },
  ]},
  { title: 'Concepts', open: true, items: [
    { label: 'Events',                   href: '/docs/concepts/streams/' },
    { label: 'Tables',                   href: '/docs/concepts/tables/' },
    { label: 'Push and fetch features',  href: '/docs/get-started/push-events/' },
  ]},
  { title: 'Community', open: false, items: [
    { label: 'Roadmap',              href: '/docs/community/roadmap/' },
    { label: 'Contributing',         href: '/docs/community/contributing/' },
    { label: 'Discussions',          href: 'https://github.com/beava-dev/beava/discussions', external: true },
  ]},
];

const DocsSidebar = ({ active = '' }) => {
  // Initial open state: per-section default; auto-expand a section if the
  // active page label matches one of its items.
  const initialOpen = {};
  SECTIONS.forEach(sec => {
    const containsActive = active && sec.items.some(it => it.label === active);
    initialOpen[sec.title] = containsActive || sec.open === true;
  });
  const [open, setOpen] = React.useState(initialOpen);
  const [drawerOpen, setDrawerOpen] = React.useState(false);
  const toggle = (title) => setOpen(prev => ({ ...prev, [title]: !prev[title] }));

  React.useEffect(() => {
    const onResize = () => { if (window.innerWidth >= 1000) setDrawerOpen(false); };
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  React.useEffect(() => {
    if (drawerOpen) {
      const prev = document.body.style.overflow;
      document.body.style.overflow = 'hidden';
      return () => { document.body.style.overflow = prev; };
    }
  }, [drawerOpen]);

  const sidebarBody = (
    <>
      {SECTIONS.map(sec => {
        const isOpen = open[sec.title];
        const isEmpty = sec.items.length === 0;
        return (
          <div key={sec.title} className={'bv-side-group' + (isOpen ? ' open' : '') + (isEmpty ? ' empty' : '')}>
            <div className="bv-side-head" onClick={() => !isEmpty && toggle(sec.title)} role={isEmpty ? undefined : 'button'} aria-expanded={isOpen}>
              <span>{sec.title}</span>
              <Chev/>
            </div>
            {sec.items.length > 0 ? (
              <ul>
                {sec.items.map(it => (
                  <li key={it.label} className={it.label === active ? 'active' : ''}>
                    <a href={it.href} {...(it.external ? { target: '_blank', rel: 'noopener' } : {})} onClick={() => setDrawerOpen(false)}>{it.label}</a>
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        );
      })}
    </>
  );

  return (
    <>
      <style>{`
        .bv-side-mobile-trigger { display: none; }
        .bv-side-drawer-overlay, .bv-side-drawer { display: none; }
        @media (max-width: 1000px) {
          .bv-side-mobile-trigger {
            display: inline-flex; align-items: center; gap: 8px;
            position: sticky; top: 60px; z-index: 40;
            margin: 0 -16px 16px;
            padding: 12px 18px;
            background: var(--beava-paper);
            border-bottom: 1px solid var(--border);
            font-family: var(--font-sans); font-size: 14px; font-weight: 500;
            color: var(--fg1); cursor: pointer; width: calc(100% + 32px);
            border-top: 0; border-left: 0; border-right: 0;
          }
          .bv-side-mobile-trigger svg { color: var(--fg3); }
          .bv-side-drawer-overlay {
            display: block; position: fixed; inset: 0; z-index: 90;
            background: rgba(26,23,20,0.5); backdrop-filter: blur(4px);
          }
          .bv-side-drawer {
            display: block; position: fixed; top: 0; left: 0; bottom: 0; z-index: 91;
            width: 86vw; max-width: 360px;
            background: var(--bg);
            border-right: 1px solid var(--border);
            box-shadow: 4px 0 24px rgba(26,23,20,0.18);
            padding: 18px 16px 32px;
            overflow-y: auto;
          }
          .bv-side-drawer .bv-side-drawer-close {
            display: flex; align-items: center; justify-content: space-between;
            padding: 6px 8px 14px; border-bottom: 1px solid var(--border);
            margin-bottom: 8px;
            font-family: var(--font-sans); font-size: 13px; font-weight: 600;
            color: var(--fg3); text-transform: uppercase; letter-spacing: 0.06em;
          }
          .bv-side-drawer .bv-side-drawer-close button {
            background: transparent; border: 1px solid var(--border);
            border-radius: 6px; padding: 4px 8px; cursor: pointer; color: var(--fg2);
          }
        }
      `}</style>

      <button className="bv-side-mobile-trigger" onClick={() => setDrawerOpen(true)} aria-label="Open docs menu">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
          <path d="M3 6h18M3 12h18M3 18h18"/>
        </svg>
        <span>Docs menu</span>
        {active && <span style={{ color: 'var(--fg3)', marginLeft: 'auto' }}>· {active}</span>}
      </button>

      <aside className="bv-side">
        {sidebarBody}
      </aside>

      {drawerOpen && (
        <>
          <div className="bv-side-drawer-overlay" onClick={() => setDrawerOpen(false)}/>
          <aside className="bv-side bv-side-drawer">
            <div className="bv-side-drawer-close">
              <span>Docs menu</span>
              <button onClick={() => setDrawerOpen(false)} aria-label="Close menu">Close ✕</button>
            </div>
            {sidebarBody}
          </aside>
        </>
      )}
    </>
  );
};
// Flat list of internal pages in sidebar order — used by <Pager auto/> to
// auto-resolve prev/next without each page hardcoding neighbors. Skip
// external links (they aren't navigable as page chain).
window.FLAT_SIDEBAR = SECTIONS.flatMap(sec =>
  sec.items.filter(it => !it.external).map(it => ({ label: it.label, href: it.href }))
);

window.DocsSidebar = DocsSidebar;
