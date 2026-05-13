// ui_kits/_shared/SiteHeader.jsx
//
// One header for the whole site. Marketing and docs share brand, link set,
// and right-end action; docs adds a centered search slot. The active page
// is shown by an orange dot after the link label (matches docs reference).
//
// Props:
//   active    — 'guide' | 'docs' | 'community' (or null)
//   search    — true to show ⌘K bar in the middle (docs)
//   maxWidth  — inner container width (1200 marketing, 1400 docs)
//
const SiteHeader = ({ active = null, search = false, maxWidth = 1200 }) => {
  const inner = {
    maxWidth, width: '100%', margin: '0 auto',
    display: 'flex', alignItems: 'center', gap: 24,
  };

  const linkBase = {
    padding: '6px 10px', borderRadius: 7,
    fontSize: 14, fontWeight: 500,
    color: 'var(--fg2)', textDecoration: 'none',
    fontFamily: 'var(--font-sans)',
    display: 'inline-flex', alignItems: 'center', gap: 6,
    cursor: 'pointer',
  };
  const linkActive = { ...linkBase, color: 'var(--fg1)' };
  const dot = { width: 5, height: 5, borderRadius: 999, background: 'var(--accent)' };

  const Link = ({ id, label, external }) => {
    const isActive = active === id;
    return (
      <a style={isActive ? linkActive : linkBase}>
        {label}
        {isActive && <span style={dot}/>}
        {external && <span style={{ color: 'var(--fg3)', fontSize: 11 }}>↗</span>}
      </a>
    );
  };

  return (
    <header style={{
      position: 'sticky', top: 0, zIndex: 50,
      height: 60,
      background: 'rgba(253,250,244,0.92)',
      backdropFilter: 'blur(10px)',
      borderBottom: '1px solid var(--border)',
      display: 'flex', alignItems: 'center',
      padding: '0 24px',
    }}>
      <div style={inner}>
        {/* brand */}
        <a href="#" style={{ display: 'inline-flex', alignItems: 'center', gap: 10, textDecoration: 'none', color: 'var(--fg1)' }}>
          <img src="../../assets/mascot-mark-geometric-transparent.png" alt="" width={32} height={32} style={{ display: 'block' }}/>
          <span style={{ fontFamily: 'var(--font-serif)', fontWeight: 600, fontStyle: 'italic', fontSize: 26, letterSpacing: '-0.025em', lineHeight: 1 }}>beava</span>
          <span style={{
            fontFamily: 'var(--font-mono)', fontSize: 10.5, fontWeight: 600,
            color: 'var(--accent)', background: 'var(--beava-orange-wash)',
            padding: '2px 6px', borderRadius: 999, letterSpacing: '0.02em',
            marginLeft: 2,
          }}>v0.14</span>
        </a>

        {/* search slot — docs only */}
        {search ? (
          <div style={{
            flex: 1, maxWidth: 360, margin: '0 auto',
            display: 'inline-flex', alignItems: 'center', gap: 8,
            height: 32, padding: '0 10px',
            background: '#fff',
            border: '1px solid var(--border)', borderRadius: 8,
            color: 'var(--fg3)', fontSize: 13, cursor: 'text',
            fontFamily: 'var(--font-sans)',
          }}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="11" cy="11" r="7"/><path d="M21 21l-4.35-4.35"/></svg>
            <span style={{ flex: 1 }}>Search the docs</span>
            <span style={{
              fontFamily: 'var(--font-mono)', fontSize: 11,
              color: 'var(--fg3)', background: 'var(--beava-paper)',
              border: '1px solid var(--border)', borderRadius: 5,
              padding: '1px 6px', lineHeight: 1,
            }}>⌘K</span>
          </div>
        ) : <div style={{ flex: 1 }}/>}

        {/* nav links */}
        <nav style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
          <Link id="quickstart" label="Quickstart"/>
          <Link id="docs" label="Docs"/>
          <Link id="examples" label="Examples"/>
          <Link id="github" label="GitHub" external/>
        </nav>

        {/* right-end action — github stars */}
        <a href="#" style={{
          display: 'inline-flex', alignItems: 'center', gap: 6,
          height: 30, padding: '0 10px',
          fontSize: 12.5, fontWeight: 600, color: 'var(--fg1)',
          background: '#fff',
          border: '1px solid var(--border)', borderRadius: 8,
          textDecoration: 'none',
          fontFamily: 'var(--font-sans)',
        }}>
          <svg width="13" height="13" viewBox="0 0 24 24" fill="var(--accent)"><path d="M12 2a10 10 0 0 0-3.16 19.49c.5.09.68-.22.68-.48v-1.7c-2.78.6-3.37-1.34-3.37-1.34-.46-1.16-1.11-1.47-1.11-1.47-.91-.62.07-.6.07-.6 1 .07 1.53 1.03 1.53 1.03.89 1.53 2.34 1.09 2.91.83.09-.65.35-1.09.63-1.34-2.22-.25-4.56-1.11-4.56-4.94 0-1.09.39-1.98 1.03-2.68-.1-.26-.45-1.27.1-2.65 0 0 .84-.27 2.75 1.02a9.56 9.56 0 0 1 5 0c1.91-1.29 2.75-1.02 2.75-1.02.55 1.38.2 2.39.1 2.65.64.7 1.03 1.59 1.03 2.68 0 3.84-2.34 4.68-4.57 4.93.36.31.68.92.68 1.85v2.75c0 .27.18.58.69.48A10 10 0 0 0 12 2z"/></svg>
          <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--fg2)' }}>8.2k</span>
        </a>
      </div>
    </header>
  );
};
window.SiteHeader = SiteHeader;
