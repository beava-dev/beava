// ui_kits/marketing/Footer.jsx
const FOOTER_COLS = [
  { title: 'Product',   links: ['Features', 'Docs', 'Changelog', 'Roadmap', 'Pricing'] },
  { title: 'Learn',     links: ['Field guide', 'Blog', 'Examples', 'Reference'] },
  { title: 'Community', links: ['GitHub', 'Discord', 'Talks', 'Brand kit'] },
];

const Footer = () => (
  <footer style={{ background: 'var(--bg-alt)', borderTop: '1px solid var(--border)', padding: '64px 24px 40px' }}>
    <div style={{ maxWidth: 1200, margin: '0 auto' }}>
      <div style={{ display: 'grid', gridTemplateColumns: '1.4fr 1fr 1fr 1fr', gap: 40, marginBottom: 48 }}>
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 14 }}>
            <img src="../../assets/mascot-mark-geometric-transparent.png" width={36} height={36} style={{ display: 'block' }}/>
            <span style={{ fontFamily: 'var(--font-serif)', fontWeight: 600, fontStyle: 'italic', fontSize: 26, letterSpacing: '-0.025em', lineHeight: 1, color: 'var(--fg1)' }}>beava</span>
          </div>
          <p style={{ fontFamily: 'var(--font-sans)', fontSize: 14, lineHeight: 1.55, color: 'var(--fg3)', margin: '0 0 14px', maxWidth: 280 }}>
            Open-source feature server for real-time features. One binary. No Kafka.
          </p>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--fg3)' }}>
            Apache 2.0 · built by a small team
          </div>
        </div>
        {FOOTER_COLS.map(col => (
          <div key={col.title}>
            <div style={{ fontFamily: 'var(--font-sans)', fontSize: 12, fontWeight: 700, textTransform: 'uppercase', letterSpacing: '0.08em', color: 'var(--fg3)', marginBottom: 12 }}>{col.title}</div>
            <ul style={{ listStyle: 'none', padding: 0, margin: 0, display: 'flex', flexDirection: 'column', gap: 8 }}>
              {col.links.map(l => (
                <li key={l}><a style={{ fontFamily: 'var(--font-sans)', fontSize: 14, color: 'var(--fg2)', textDecoration: 'none' }}>{l}</a></li>
              ))}
            </ul>
          </div>
        ))}
      </div>
      <div style={{ borderTop: '1px solid var(--border)', paddingTop: 20, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <div style={{ fontFamily: 'var(--font-sans)', fontSize: 13, color: 'var(--fg3)' }}>© 2026 beava labs · beava.dev</div>
        <div style={{ display: 'flex', gap: 16 }}>
          <a style={{ color: 'var(--fg3)' }}><Icon name="github" size={18}/></a>
        </div>
      </div>
    </div>
  </footer>
);
window.Footer = Footer;
