// js/sdk/SdkTOC.jsx
//
// Auto-built right-rail "On this page" TOC. Scans <main> for h2[id]
// and h3[id] on mount and renders them as anchored links. Optional
// IntersectionObserver activates the entry whose section is in view.
//
// Mount:
//   <div id="bv-sdk-toc" class="toc" data-edit-path="sdk/python/app/index.html"></div>
//   ...
//   <script type="text/babel" src="/js/sdk/SdkTOC.jsx"></script>
//   <script type="text/babel">
//     ReactDOM.createRoot(document.getElementById('bv-sdk-toc'))
//       .render(<SdkTOC editPath="sdk/python/app/index.html"/>);
//   </script>
//
// Props:
//   editPath — path under beava-website/project/ for the page's
//              source HTML, used in the "Edit on GitHub" link.

const SdkTOC = ({ editPath = '' }) => {
  const [items, setItems]   = React.useState([]);
  const [active, setActive] = React.useState(null);

  React.useEffect(() => {
    // Pull h2/h3 with id attrs from <main>. Method-headers (.method-head)
    // wrap a <h3> sibling in some pages; either form is fine — we just
    // scan tag + id.
    const main = document.querySelector('main.content');
    if (!main) return;
    const heads = main.querySelectorAll('h2[id], h3[id]');
    const list = Array.from(heads).map(el => ({
      id: el.id,
      label: (el.dataset.tocLabel || el.textContent || el.id).trim(),
      level: el.tagName === 'H3' ? 3 : 2,
    }));
    setItems(list);

    if (list.length === 0) return;
    if (typeof IntersectionObserver === 'undefined') return;
    const observed = new Map();
    const onIntersect = entries => {
      // Track which sections are currently intersecting; pick the
      // top-most one (lowest boundingClientRect.top above 88px) as
      // active. This mirrors the natural reading position.
      entries.forEach(e => observed.set(e.target.id, e));
      const inView = Array.from(observed.values())
        .filter(e => e.isIntersecting)
        .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top);
      if (inView.length > 0) setActive(inView[0].target.id);
    };
    const obs = new IntersectionObserver(onIntersect, {
      rootMargin: '-88px 0px -60% 0px',
      threshold: 0,
    });
    heads.forEach(h => obs.observe(h));
    return () => obs.disconnect();
  }, []);

  const editHref = editPath
    ? `https://github.com/beava-dev/beava/edit/main/beava-website/project/${editPath}`
    : null;

  return (
    <React.Fragment>
      <div className="kicker">On this page</div>
      <ul>
        {items.map(it => (
          <li key={it.id}
              className={[
                it.level === 3 ? 'h3' : 'h2',
                active === it.id ? 'active' : '',
              ].filter(Boolean).join(' ')}>
            <a href={`#${it.id}`}>{it.label}</a>
          </li>
        ))}
      </ul>
      <div className="footer-links">
        {editHref && (
          <a href={editHref} target="_blank" rel="noopener">
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 20h9"/>
              <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"/>
            </svg>
            Edit on GitHub
          </a>
        )}
        <a href="https://discord.gg/Jnx89PN9" target="_blank" rel="noopener">
          <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M21 11.5a8.38 8.38 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.38 8.38 0 0 1-3.8-.9L3 21l1.9-5.7a8.38 8.38 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.38 8.38 0 0 1 3.8-.9h.5a8.48 8.48 0 0 1 8 8v.5z"/>
          </svg>
          Ask in Discord
        </a>
      </div>
    </React.Fragment>
  );
};

window.SdkTOC = SdkTOC;
