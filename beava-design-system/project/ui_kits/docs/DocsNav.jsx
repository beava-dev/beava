// ui_kits/docs/DocsNav.jsx
const DocsNav = () => (
  <header style={{
    position: 'sticky', top: 0, zIndex: 50,
    height: 60, background: 'rgba(253,250,244,0.92)', backdropFilter: 'blur(10px)',
    borderBottom: '1px solid var(--border)', display: 'flex', alignItems: 'center',
    padding: '0 24px'
  }}>
    <div style={{ maxWidth: 1400, width: '100%', margin: '0 auto', display: 'flex', alignItems: 'center', gap: 20 }}>
      <a style={{ display: 'flex', alignItems: 'center', gap: 10, textDecoration: 'none', color: 'var(--fg1)' }}>
        <img src="../../assets/mascot-mark-geometric-transparent.png" width={30} height={30} style={{ display: 'block' }}/>
        <span style={{ fontFamily: 'var(--font-serif)', fontWeight: 600, fontStyle: 'italic', fontSize: 22, letterSpacing: '-0.025em', lineHeight: 1 }}>beava</span>
        <span style={{ fontFamily: 'var(--font-sans)', fontSize: 14, color: 'var(--fg3)', padding: '2px 8px', background: 'var(--beava-paper)', borderRadius: 6, border: '1px solid var(--border)', marginLeft: 4 }}>docs</span>
      </a>
      <div style={{ display: 'flex', gap: 2, marginLeft: 20, flex: 1 }}>
        {['Docs','Learn','Blog','Community'].map((l,i) => (
          <a key={l} style={{ padding: '6px 10px', borderRadius: 8, fontSize: 14, color: i===0 ? 'var(--accent)' : 'var(--fg2)', textDecoration: 'none', fontWeight: 500, fontFamily: 'var(--font-sans)' }}>{l}</a>
        ))}
      </div>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, background: 'var(--beava-paper)', border: '1px solid var(--border)', borderRadius: 10, padding: '7px 12px', width: 280, color: 'var(--fg3)', fontSize: 13, fontFamily: 'var(--font-sans)' }}>
        <DIcon name="search" size={14}/>
        <span style={{ flex: 1 }}>Search docs…</span>
        <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, padding: '2px 6px', border: '1px solid var(--border)', borderRadius: 4, background: '#fff' }}>⌘K</span>
      </div>
      <a style={{ color: 'var(--fg2)', display: 'flex', alignItems: 'center', padding: 6 }}><DIcon name="github" size={18}/></a>
    </div>
  </header>
);
window.DocsNav = DocsNav;
