// ui_kits/marketing/FinalCTA.jsx
// Section 5 — install (repeat) + GitHub star + Chapter 1 link + Discussions.

const FinalCTA = () => {
  const [tabId, setTabId] = React.useState('brew');
  const [copied, setCopied] = React.useState(false);
  const TABS = [
    { id: 'brew',   cmd: 'brew install beava' },
    { id: 'curl',   cmd: 'curl -fsSL beava.dev/install.sh | sh' },
    { id: 'docker', cmd: 'docker run -p 6400:6400 beava/beava:latest' },
  ];
  const tab = TABS.find(t => t.id === tabId);
  const copy = () => {
    navigator.clipboard?.writeText(tab.cmd);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  return (
    <section style={{
      padding: '96px 24px',
      background: 'var(--beava-cream-deep)',
      borderTop: '1px solid var(--border)',
    }}>
      <div style={{ maxWidth: 920, margin: '0 auto', textAlign: 'center', position: 'relative' }}>
        {/* Brand lockup — geometric mascot + serif italic wordmark */}
        <div style={{
          display: 'inline-flex', alignItems: 'center', gap: 18,
          marginBottom: 32,
        }}>
          <img
            src="../../assets/mascot-mark-geometric-transparent.png"
            alt=""
            width={88}
            height={88}
            style={{ display: 'block' }}
          />
          <span style={{
            fontFamily: 'var(--font-serif)', fontWeight: 600, fontStyle: 'italic',
            fontSize: 84, letterSpacing: '-0.025em', lineHeight: 0.95,
            color: 'var(--fg1)',
          }}>beava</span>
        </div>

        <Eyebrow>Three ways in</Eyebrow>
        <h2 style={{
          fontFamily: 'var(--font-serif)', fontWeight: 600,
          fontSize: 'clamp(36px, 4.5vw, 52px)', lineHeight: 1.1,
          letterSpacing: '-0.02em', color: 'var(--fg1)',
          margin: '12px 0 16px', textWrap: 'balance',
        }}>
          Install it. Read Chapter 1. Or just lurk.
        </h2>
        <p style={{
          fontFamily: 'var(--font-sans)', fontSize: 17, lineHeight: 1.55,
          color: 'var(--fg2)', margin: '0 auto 36px', maxWidth: 620,
          textWrap: 'pretty',
        }}>
          The same binary, the same source. Nothing to sign up for.
        </p>

        {/* Install tabs (compact, repeat) */}
        <div style={{ maxWidth: 560, margin: '0 auto 36px' }}>
          <div style={{ display: 'flex', gap: 2, justifyContent: 'center', marginBottom: 0 }}>
            {TABS.map(t => {
              const active = t.id === tabId;
              return (
                <button key={t.id} onClick={() => setTabId(t.id)} style={{
                  fontFamily: 'var(--font-mono)', fontSize: 12.5,
                  padding: '6px 14px 8px',
                  background: active ? 'var(--code-bg)' : 'transparent',
                  color: active ? 'var(--fg1)' : 'var(--fg3)',
                  border: '1px solid',
                  borderColor: active ? 'var(--border)' : 'transparent',
                  borderBottom: active ? '1px solid var(--code-bg)' : '1px solid var(--border)',
                  borderRadius: '8px 8px 0 0',
                  cursor: 'pointer', fontWeight: active ? 600 : 500,
                  position: 'relative', top: 1,
                }}>{t.id}</button>
              );
            })}
          </div>
          <div style={{
            display: 'flex', alignItems: 'center', gap: 12,
            background: 'var(--code-bg)', border: '1px solid var(--border)', borderTop: 'none',
            borderRadius: '0 10px 10px 10px',
            padding: '12px 16px',
          }}>
            <span style={{ color: 'var(--accent)', fontFamily: 'var(--font-mono)', fontSize: 14 }}>$</span>
            <code style={{
              fontFamily: 'var(--font-mono)', fontSize: 14, color: 'var(--code-fg)',
              flex: 1, textAlign: 'left',
            }}>{tab.cmd}</code>
            <button onClick={copy} style={{
              fontFamily: 'var(--font-mono)', fontSize: 12,
              padding: '4px 10px',
              background: copied ? 'var(--beava-success-wash)' : '#fff',
              color: copied ? 'var(--beava-success)' : 'var(--fg2)',
              border: '1px solid',
              borderColor: copied ? '#cdd9b6' : 'var(--border)',
              borderRadius: 6, cursor: 'pointer', fontWeight: 600,
              display: 'inline-flex', alignItems: 'center', gap: 5,
            }}>
              {copied ? <><Icon name="check" size={11}/> copied</> : <><Icon name="copy" size={11}/> copy</>}
            </button>
          </div>
        </div>

        {/* Three-way fork */}
        <div style={{
          display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 14,
          textAlign: 'left',
        }} className="beava-finalcta-grid">
          {[
            {
              kicker: 'guidebook',
              title: 'Read Chapter 1',
              body: 'A 10-minute live build that turns into a per-customer analytics dashboard. No setup beyond the install above.',
              cta: 'Start Chapter 1 \u2192',
              accent: 'var(--accent)',
            },
            {
              kicker: 'github',
              title: 'Star on GitHub',
              body: 'Apache 2.0. Issues are open. Releases are signed.',
              cta: 'github.com/beava-dev/beava \u2192',
              accent: 'var(--fg1)',
            },
            {
              kicker: 'community',
              title: 'GitHub Discussions',
              body: 'Half the answers in our docs started as someone asking "is this a bug?" out loud.',
              cta: 'Join the discussion \u2192',
              accent: 'var(--beava-info)',
            },
          ].map(c => (
            <a key={c.kicker} href="#" style={{
              background: '#fff', border: '1px solid var(--border)', borderRadius: 14,
              padding: '20px 22px',
              textDecoration: 'none',
              display: 'flex', flexDirection: 'column', gap: 14,
              transition: 'all 200ms cubic-bezier(0.22,1,0.36,1)',
              boxShadow: '0 1px 2px rgba(26,23,20,0.04)',
            }}
            onMouseEnter={e => {
              e.currentTarget.style.transform = 'translateY(-2px)';
              e.currentTarget.style.boxShadow = '0 4px 16px rgba(26,23,20,0.08)';
            }}
            onMouseLeave={e => {
              e.currentTarget.style.transform = '';
              e.currentTarget.style.boxShadow = '0 1px 2px rgba(26,23,20,0.04)';
            }}>
              <div style={{
                fontFamily: 'var(--font-sans)', fontSize: 11, fontWeight: 600,
                textTransform: 'uppercase', letterSpacing: '0.08em',
                color: c.accent,
              }}>{c.kicker}</div>
              <div style={{
                fontFamily: 'var(--font-serif)', fontWeight: 600, fontSize: 19,
                lineHeight: 1.25, color: 'var(--fg1)', textWrap: 'balance',
              }}>{c.title}</div>
              <p style={{
                fontFamily: 'var(--font-sans)', fontSize: 14, lineHeight: 1.5,
                color: 'var(--fg2)', margin: 0, flex: 1,
              }}>{c.body}</p>
              <div style={{
                fontFamily: 'var(--font-sans)', fontSize: 13, fontWeight: 500,
                color: c.accent, marginTop: 4,
              }}>{c.cta}</div>
            </a>
          ))}
        </div>
      </div>

      <style>{`
        @media (max-width: 760px) {
          .beava-finalcta-grid {
            grid-template-columns: 1fr !important;
          }
          .beava-finalcta-mascot {
            display: none !important;
          }
        }
      `}</style>
    </section>
  );
};
window.FinalCTA = FinalCTA;
