// js/_shared/SiteFooter.jsx
//
// Canonical site footer. ONE component, every page.
// Ported from beava-design-system/project/ui_kits/_shared/SiteFooter.jsx
// with absolute paths and real hrefs.
//
const SITE_FOOTER_COLS = [
  { title: 'Project', links: [
    { label: 'Guide',           href: '/guide/' },
    { label: 'Docs',            href: '/docs/' },
    { label: 'Roadmap',         href: '/docs/community/roadmap/' },
    { label: 'OSS commitment',  href: '/docs/vision/open-source/' },
  ]},
  { title: 'Community', links: [
    { label: 'GitHub',      href: 'https://github.com/beava-dev/beava', external: true },
    { label: 'Discussions', href: 'https://github.com/beava-dev/beava/discussions', external: true },
    { label: 'Contributing', href: '/docs/community/contributing/' },
  ]},
];

const SiteFooter = ({ maxWidth = 1200 }) => (
  <footer className="bv-site-footer" style={{ background: 'var(--bg-alt)', borderTop: '1px solid var(--border)', padding: '64px 24px 40px' }}>
    <style>{`
      @media (max-width: 760px) {
        .bv-site-footer .bv-foot-grid {
          grid-template-columns: 1fr !important;
          gap: 32px !important;
        }
        .bv-site-footer .bv-foot-bottom {
          flex-direction: column !important;
          align-items: flex-start !important;
          gap: 16px !important;
        }
      }
    `}</style>
    <div style={{ maxWidth, margin: '0 auto' }}>
      <div className="bv-foot-grid" style={{ display: 'grid', gridTemplateColumns: '1.6fr 1fr 1fr', gap: 40, marginBottom: 48 }}>
        <div>
          <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 14 }}>
            <img src="/assets/mascot-mark-geometric-transparent.png" width={36} height={36} alt="" style={{ display: 'block' }}/>
            <span style={{ fontFamily: 'var(--font-serif)', fontWeight: 600, fontStyle: 'italic', fontSize: 28, letterSpacing: '-0.025em', lineHeight: 1, color: 'var(--fg1)' }}>beava</span>
            <span style={{
              fontFamily: 'var(--font-mono)', fontSize: 10.5, fontWeight: 600,
              color: 'var(--accent)', background: 'var(--beava-orange-wash)',
              padding: '2px 6px', borderRadius: 999, letterSpacing: '0.02em',
            }}>v0</span>
          </div>
          <p style={{ fontFamily: 'var(--font-sans)', fontSize: 14, lineHeight: 1.55, color: 'var(--fg3)', margin: '0 0 14px', maxWidth: 280 }}>
            Open-source feature server for real-time features. One binary. No Kafka.
          </p>
          <div style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--fg3)' }}>
            Apache 2.0 · built by a small team
          </div>
        </div>
        {SITE_FOOTER_COLS.map(col => (
          <div key={col.title}>
            <div style={{ fontFamily: 'var(--font-sans)', fontSize: 12, fontWeight: 700, textTransform: 'uppercase', letterSpacing: '0.08em', color: 'var(--fg3)', marginBottom: 12 }}>{col.title}</div>
            <ul style={{ listStyle: 'none', padding: 0, margin: 0, display: 'flex', flexDirection: 'column', gap: 2 }}>
              {col.links.map(l => (
                <li key={l.label}>
                  <a href={l.href}
                     {...(l.external ? { target: '_blank', rel: 'noopener' } : {})}
                     style={{ fontFamily: 'var(--font-sans)', fontSize: 14, color: 'var(--fg2)', textDecoration: 'none', display: 'inline-block', padding: '5px 0' }}>
                    {l.label}
                  </a>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
      <div className="bv-foot-bottom" style={{ borderTop: '1px solid var(--border)', paddingTop: 20, display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
        <div style={{ fontFamily: 'var(--font-sans)', fontSize: 13, color: 'var(--fg3)' }}>© 2026 beava labs · beava.dev</div>
        <div style={{ display: 'flex', gap: 16, alignItems: 'center' }}>
          <span style={{
            display: 'inline-flex', alignItems: 'center', gap: 6,
            fontFamily: 'var(--font-mono)', fontSize: 11.5, color: 'var(--fg3)',
            padding: '4px 10px', borderRadius: 999,
            border: '1px solid var(--border)', background: '#fff',
          }}>
            <span style={{ width: 6, height: 6, borderRadius: 999, background: '#19a974', boxShadow: '0 0 0 3px rgba(25,169,116,0.18)' }}/>
            all systems operational
          </span>
          <a href="https://github.com/beava-dev/beava" target="_blank" rel="noopener" style={{ color: 'var(--fg3)' }}>
            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2a10 10 0 0 0-3.16 19.49c.5.09.68-.22.68-.48v-1.7c-2.78.6-3.37-1.34-3.37-1.34-.46-1.16-1.11-1.47-1.11-1.47-.91-.62.07-.6.07-.6 1 .07 1.53 1.03 1.53 1.03.89 1.53 2.34 1.09 2.91.83.09-.65.35-1.09.63-1.34-2.22-.25-4.56-1.11-4.56-4.94 0-1.09.39-1.98 1.03-2.68-.1-.26-.45-1.27.1-2.65 0 0 .84-.27 2.75 1.02a9.56 9.56 0 0 1 5 0c1.91-1.29 2.75-1.02 2.75-1.02.55 1.38.2 2.39.1 2.65.64.7 1.03 1.59 1.03 2.68 0 3.84-2.34 4.68-4.57 4.93.36.31.68.92.68 1.85v2.75c0 .27.18.58.69.48A10 10 0 0 0 12 2z"/></svg>
          </a>
        </div>
      </div>

      {/* LLM-agent affordance — site-wide. Two-tier per llmstxt.org:
          /sdk/llms.txt is the short structured index (~6 KB),
          /sdk/llms-full.txt is the full plain-text concatenation of
          all SDK pages (~145 KB). Both generated by
          beava-website/scripts/build-llms-txt.mjs. */}
      <div className="bv-llm-footer" style={{
        marginTop: 18,
        paddingTop: 14,
        borderTop: '1px dashed var(--border)',
        fontFamily: 'var(--font-sans)',
        fontSize: 12, lineHeight: 1.55,
        color: 'var(--fg3)',
        textAlign: 'center',
      }}>
        <span style={{
          fontFamily: 'var(--font-mono)', fontSize: 10.5, fontWeight: 700,
          textTransform: 'uppercase', letterSpacing: '0.06em',
          color: 'var(--accent)', marginRight: 8,
        }}>
          If you're an agent
        </span>
        SDK reference for LLMs:{' '}
        <a href="/sdk/llms.txt" style={{
          color: 'var(--accent)',
          fontFamily: 'var(--font-mono)',
          fontSize: '0.95em',
        }}>/sdk/llms.txt</a>{' '}(short index) ·{' '}
        <a href="/sdk/llms-full.txt" style={{
          color: 'var(--accent)',
          fontFamily: 'var(--font-mono)',
          fontSize: '0.95em',
        }}>/sdk/llms-full.txt</a>{' '}(full text).
      </div>
    </div>
  </footer>
);
window.SiteFooter = SiteFooter;
