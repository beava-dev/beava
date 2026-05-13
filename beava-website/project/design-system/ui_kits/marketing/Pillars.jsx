// ui_kits/marketing/Pillars.jsx
// Section 2 — names the value prop before the demo (PipelineShowcase) below.
// Eyebrow copy from locked decision #14: "Stream processing shouldn't require
// a platform team."

const PILLARS = [
  {
    kicker: '01',
    title: 'Replaces Redis + Lua',
    body: 'No more 200-line Lua scripts maintaining counters. Beava is the table.',
    foot: 'redis-cli → bv.get(...)',
  },
  {
    kicker: '02',
    title: 'No streaming stack',
    body: 'No Kafka, no Flink, no Schema Registry. One Go binary, one HTTP port.',
    foot: '~4 MB · single binary',
  },
  {
    kicker: '03',
    title: 'Crash-safe by default',
    body: 'WAL on every write. Restart mid-flight; tables come back exactly where they were.',
    foot: 'fsync = on, always',
  },
  {
    kicker: '04',
    title: 'Apache 2.0, forever',
    body: 'No source-available rug-pull. No usage limits. The same binary in the cloud you self-host.',
    foot: 'github.com/beava-dev/beava',
  },
];

const Pillars = () => (
  <section style={{
    padding: '88px 24px',
    background: 'var(--bg-alt)',
    borderTop: '1px solid var(--border)',
    borderBottom: '1px solid var(--border)',
    position: 'relative', overflow: 'hidden',
  }}>
    {/* Mascot watermark — geometric mark, low-opacity, decorative */}
    <img
      src="../../assets/mascot-mark-geometric.png"
      alt=""
      width={220}
      height={220}
      style={{
        position: 'absolute', top: 24, right: -40,
        opacity: 0.06, pointerEvents: 'none',
        transform: 'rotate(8deg)',
      }}
    />
    <div style={{ maxWidth: 1200, margin: '0 auto', position: 'relative' }}>
      <div style={{ textAlign: 'center', marginBottom: 56 }}>
        <Eyebrow>Stream processing shouldn&rsquo;t require a platform team.</Eyebrow>
        <h2 style={{
          fontFamily: 'var(--font-serif)', fontWeight: 600,
          fontSize: 44, lineHeight: 1.1, letterSpacing: '-0.02em',
          color: 'var(--fg1)', margin: '12px 0 0',
          maxWidth: 720, marginLeft: 'auto', marginRight: 'auto',
          textWrap: 'balance',
        }}>
          What&rsquo;s different
        </h2>
      </div>

      <div style={{
        display: 'grid',
        gridTemplateColumns: 'repeat(4, 1fr)',
        gap: 16,
      }} className="beava-pillars-grid">
        {PILLARS.map(p => (
          <div key={p.title} style={{
            background: '#fff',
            border: '1px solid var(--border)',
            borderRadius: 14,
            padding: '24px 22px 20px',
            boxShadow: '0 1px 2px rgba(26,23,20,0.04), 0 2px 6px rgba(26,23,20,0.06)',
            display: 'flex', flexDirection: 'column', gap: 12,
            position: 'relative',
            transition: 'all 200ms cubic-bezier(0.22,1,0.36,1)',
          }}
          onMouseEnter={e => {
            e.currentTarget.style.transform = 'translateY(-2px)';
            e.currentTarget.style.boxShadow = '0 2px 4px rgba(26,23,20,0.04), 0 8px 20px rgba(26,23,20,0.08)';
          }}
          onMouseLeave={e => {
            e.currentTarget.style.transform = '';
            e.currentTarget.style.boxShadow = '0 1px 2px rgba(26,23,20,0.04), 0 2px 6px rgba(26,23,20,0.06)';
          }}>
            <div style={{
              fontFamily: 'var(--font-mono)', fontSize: 12, fontWeight: 600,
              color: 'var(--accent)', letterSpacing: '0.02em',
            }}>{p.kicker}</div>

            <h3 style={{
              fontFamily: 'var(--font-sans)', fontWeight: 600,
              fontSize: 13, textTransform: 'uppercase', letterSpacing: '0.06em',
              color: 'var(--fg1)', margin: 0, lineHeight: 1.3,
            }}>{p.title}</h3>

            <p style={{
              fontFamily: 'var(--font-sans)', fontSize: 15, lineHeight: 1.55,
              color: 'var(--fg2)', margin: 0, textWrap: 'pretty',
              flex: 1,
            }}>{p.body}</p>

            <div style={{
              paddingTop: 12, marginTop: 4,
              borderTop: '1px dashed var(--border)',
              fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--fg3)',
              lineHeight: 1.4,
            }}>{p.foot}</div>
          </div>
        ))}
      </div>
    </div>

    <style>{`
      @media (max-width: 960px) {
        .beava-pillars-grid {
          grid-template-columns: repeat(2, 1fr) !important;
        }
      }
      @media (max-width: 560px) {
        .beava-pillars-grid {
          grid-template-columns: 1fr !important;
        }
      }
    `}</style>
  </section>
);
window.Pillars = Pillars;
