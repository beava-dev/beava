// ui_kits/marketing/Hero404.jsx
//
// 404 page hero — the rare beava surface where the mascot leads.
// Composition: huge serif "4 [mascot] 4". The middle "0" is the floating
// mascot gif, sitting in a cream paper disc. Spring-eased bob on hover.
//
const Hero404 = () => {
  // The path the visitor tried — read from URL on mount so it feels real
  // without server cooperation. Fallback to a plausible-looking value.
  const [attempted, setAttempted] = React.useState('/docs/rolling-counters/redis-compat');
  React.useEffect(() => {
    try {
      const p = window.location.pathname;
      if (p && p !== '/' && !p.endsWith('404.html')) setAttempted(p);
    } catch (e) {}
  }, []);

  // Hover-bounce on the mascot — the only spring on the page.
  const [hovered, setHovered] = React.useState(false);

  const popular = [
    { kicker: 'START HERE',  title: 'Quickstart',          meta: 'docker · brew · 3 commands', href: '#' },
    { kicker: 'DOCS',        title: 'Rolling counters',    meta: 'the worked example',          href: '#' },
    { kicker: 'FIELD GUIDE', title: 'The real-time guide', meta: 'chapter index · 6 chapters',  href: '#' },
  ];

  return (
    <main style={{
      position: 'relative',
      minHeight: 'calc(100vh - 60px)',
      display: 'flex', flexDirection: 'column',
      alignItems: 'center', justifyContent: 'flex-start',
      padding: '64px 24px 96px',
      overflow: 'hidden',
    }}>
      {/* Local keyframes for the mascot float + arrow tick */}
      <style>{`
        @keyframes beava-404-bob {
          0%   { transform: translateY(0) rotate(-1deg); }
          50%  { transform: translateY(-10px) rotate(1deg); }
          100% { transform: translateY(0) rotate(-1deg); }
        }
        @keyframes beava-404-bob-big {
          0%, 100% { transform: translateY(0) rotate(-2deg); }
          40%      { transform: translateY(-22px) rotate(3deg); }
          60%      { transform: translateY(-14px) rotate(-2deg); }
        }
        @keyframes beava-404-arrow {
          0%, 100% { transform: translateX(0); }
          50%      { transform: translateX(-4px); }
        }
        @keyframes beava-404-fade-in {
          from { opacity: 0; transform: translateY(8px); }
          to   { opacity: 1; transform: translateY(0); }
        }
        @media (prefers-reduced-motion: reduce) {
          .b404-mascot, .b404-arrow, .b404-section { animation: none !important; }
        }
      `}</style>

      {/* very subtle paper-grain decoration: just a soft radial wash behind the 4·4 */}
      <div aria-hidden style={{
        position: 'absolute', top: 40, left: '50%', transform: 'translateX(-50%)',
        width: 900, height: 520, pointerEvents: 'none',
        background: 'radial-gradient(closest-side, var(--beava-orange-wash), transparent 70%)',
        opacity: 0.55,
      }}/>

      {/* eyebrow */}
      <div className="b404-section" style={{
        animation: 'beava-404-fade-in 360ms var(--ease-out) both',
        fontFamily: 'var(--font-sans)', fontSize: 12, fontWeight: 600,
        textTransform: 'uppercase', letterSpacing: '0.16em',
        color: 'var(--accent)',
        position: 'relative', zIndex: 2,
        marginBottom: 28,
      }}>
        Error 404 · page not found
      </div>

      {/* the 4 [mascot] 4 — composition is the page */}
      <div style={{
        position: 'relative', zIndex: 2,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        gap: 'clamp(4px, 1vw, 16px)',
        animation: 'beava-404-fade-in 480ms 60ms var(--ease-out) both',
      }}>
        <Numeral>4</Numeral>

        {/* mascot disc — sits in for the 0 */}
        <button
          type="button"
          aria-label="Wake the beaver"
          onMouseEnter={() => setHovered(true)}
          onMouseLeave={() => setHovered(false)}
          onFocus={() => setHovered(true)}
          onBlur={() => setHovered(false)}
          style={{
            position: 'relative',
            width: 'clamp(180px, 22vw, 260px)',
            height: 'clamp(180px, 22vw, 260px)',
            margin: '0 clamp(-12px, -1vw, -6px)',
            background: 'var(--beava-paper)',
            border: '1px solid var(--border)',
            borderRadius: 999,
            boxShadow: '0 2px 0 rgba(26,23,20,0.06), 0 14px 30px rgba(26,23,20,0.10), inset 0 1px 0 rgba(255,255,255,0.7)',
            padding: 0, cursor: 'pointer',
            display: 'grid', placeItems: 'center',
            transition: 'box-shadow 200ms var(--ease-out)',
          }}
        >
          {/* concentric ring — subtle */}
          <span aria-hidden style={{
            position: 'absolute', inset: 14,
            border: '1px dashed var(--border-strong)',
            borderRadius: 999,
            opacity: 0.55,
          }}/>
          <img
            className="b404-mascot"
            src="../../assets/mascot-floating-slow.gif"
            alt="beava mascot, napping in the spot where the page used to be"
            style={{
              width: '78%', height: 'auto',
              animation: hovered
                ? 'beava-404-bob-big 700ms var(--ease-spring)'
                : 'beava-404-bob 4.5s ease-in-out infinite',
              transformOrigin: 'center bottom',
              pointerEvents: 'none',
              userSelect: 'none',
            }}
          />
          {/* Gaegu marker note tethered to the disc */}
          <span aria-hidden style={{
            position: 'absolute',
            bottom: -22,
            right: -110,
            transform: 'rotate(-6deg)',
            fontFamily: 'var(--font-accent)',
            fontWeight: 700,
            fontSize: 24,
            color: 'var(--accent)',
            whiteSpace: 'nowrap',
            pointerEvents: 'none',
          }}>
            ← off-duty
          </span>
        </button>

        <Numeral>4</Numeral>
      </div>

      {/* headline */}
      <h1 className="b404-section" style={{
        animation: 'beava-404-fade-in 480ms 140ms var(--ease-out) both',
        fontFamily: 'var(--font-serif)',
        fontWeight: 600,
        fontSize: 'clamp(34px, 4.4vw, 54px)',
        lineHeight: 1.12,
        letterSpacing: '-0.02em',
        textAlign: 'center',
        color: 'var(--fg1)',
        margin: '56px 0 14px',
        maxWidth: 720,
        textWrap: 'pretty',
        position: 'relative', zIndex: 2,
      }}>
        This page <em style={{ color: 'var(--accent)', fontStyle: 'italic' }}>slipped down the dam</em>.
      </h1>

      <p className="b404-section" style={{
        animation: 'beava-404-fade-in 480ms 200ms var(--ease-out) both',
        fontFamily: 'var(--font-sans)',
        fontSize: 17,
        lineHeight: 1.55,
        color: 'var(--fg3)',
        textAlign: 'center',
        margin: '0 0 28px',
        maxWidth: 560,
        textWrap: 'pretty',
        position: 'relative', zIndex: 2,
      }}>
        The URL doesn't match anything we serve. It may have moved, been renamed,
        or never existed. Try one of the spots below — or head back to safety.
      </p>

      {/* attempted-URL strip */}
      <div className="b404-section" style={{
        animation: 'beava-404-fade-in 480ms 260ms var(--ease-out) both',
        display: 'inline-flex', alignItems: 'center', gap: 10,
        padding: '8px 14px',
        background: '#fff',
        border: '1px solid var(--border)',
        borderRadius: 999,
        boxShadow: 'var(--shadow-xs)',
        margin: '0 0 36px',
        maxWidth: '100%',
        position: 'relative', zIndex: 2,
      }}>
        <span style={{
          fontFamily: 'var(--font-sans)', fontSize: 11, fontWeight: 700,
          textTransform: 'uppercase', letterSpacing: '0.12em',
          color: 'var(--fg3)',
        }}>You tried</span>
        <span aria-hidden style={{ color: 'var(--border-strong)' }}>·</span>
        <code style={{
          fontFamily: 'var(--font-mono)',
          fontSize: 13.5,
          color: 'var(--fg1)',
          background: 'transparent', border: 0, padding: 0,
          overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
          maxWidth: 380,
        }}>beava.dev{attempted}</code>
      </div>

      {/* CTAs */}
      <div className="b404-section" style={{
        animation: 'beava-404-fade-in 480ms 320ms var(--ease-out) both',
        display: 'flex', flexWrap: 'wrap', gap: 12,
        justifyContent: 'center',
        position: 'relative', zIndex: 2,
        marginBottom: 80,
      }}>
        <a href="/" style={{
          display: 'inline-flex', alignItems: 'center', gap: 8,
          height: 44, padding: '0 20px',
          background: 'var(--accent)',
          color: '#fff',
          fontFamily: 'var(--font-sans)', fontSize: 15, fontWeight: 600,
          borderRadius: 10,
          textDecoration: 'none',
          boxShadow: 'var(--shadow-sm)',
          transition: 'background 200ms var(--ease-out), transform 120ms var(--ease-out)',
        }}
          onMouseDown={e => e.currentTarget.style.transform = 'translateY(1px)'}
          onMouseUp={e => e.currentTarget.style.transform = 'translateY(0)'}
          onMouseLeave={e => { e.currentTarget.style.transform = 'translateY(0)'; e.currentTarget.style.background = 'var(--accent)'; }}
          onMouseEnter={e => e.currentTarget.style.background = 'var(--accent-hover)'}
        >
          Back to beava.dev <span aria-hidden>→</span>
        </a>
        <a href="#" style={{
          display: 'inline-flex', alignItems: 'center', gap: 8,
          height: 44, padding: '0 18px',
          background: '#fff',
          color: 'var(--fg1)',
          fontFamily: 'var(--font-sans)', fontSize: 15, fontWeight: 600,
          border: '1px solid var(--border)',
          borderRadius: 10,
          textDecoration: 'none',
          boxShadow: 'var(--shadow-xs)',
          transition: 'border-color 200ms var(--ease-out), transform 120ms var(--ease-out)',
        }}
          onMouseEnter={e => e.currentTarget.style.borderColor = 'var(--accent)'}
          onMouseLeave={e => e.currentTarget.style.borderColor = 'var(--border)'}
        >
          Read the docs
        </a>
        <a href="#" style={{
          display: 'inline-flex', alignItems: 'center', gap: 8,
          height: 44, padding: '0 18px',
          background: 'transparent',
          color: 'var(--fg2)',
          fontFamily: 'var(--font-sans)', fontSize: 15, fontWeight: 500,
          border: '1px solid transparent',
          borderRadius: 10,
          textDecoration: 'none',
        }}>
          Report a broken link <span aria-hidden style={{ color: 'var(--fg3)' }}>↗</span>
        </a>
      </div>

      {/* popular destinations — three paper cards, stacked vocabulary
          consistent with chapter cards. */}
      <section className="b404-section" style={{
        animation: 'beava-404-fade-in 480ms 380ms var(--ease-out) both',
        width: '100%', maxWidth: 1040,
        position: 'relative', zIndex: 2,
      }}>
        <div style={{
          display: 'flex', alignItems: 'center', gap: 12,
          marginBottom: 16, paddingLeft: 4,
        }}>
          <span style={{
            fontFamily: 'var(--font-sans)', fontSize: 12, fontWeight: 700,
            textTransform: 'uppercase', letterSpacing: '0.12em',
            color: 'var(--fg3)',
          }}>Where most readers go next</span>
          <span aria-hidden style={{
            flex: 1, height: 1, background: 'var(--border)',
          }}/>
        </div>
        <div style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(3, 1fr)',
          gap: 16,
        }}>
          {popular.map(p => (
            <a key={p.title} href={p.href} className="b404-card" style={{
              display: 'flex', flexDirection: 'column', gap: 6,
              padding: '18px 20px',
              background: 'var(--beava-paper)',
              border: '1px solid var(--border)',
              borderRadius: 14,
              textDecoration: 'none',
              transition: 'border-color 200ms var(--ease-out), transform 200ms var(--ease-out), box-shadow 200ms var(--ease-out)',
            }}
              onMouseEnter={e => {
                e.currentTarget.style.borderColor = 'var(--accent)';
                e.currentTarget.style.transform = 'translateY(-1px)';
                e.currentTarget.style.boxShadow = 'var(--shadow-md)';
                const arrow = e.currentTarget.querySelector('.b404-arrow');
                if (arrow) arrow.style.transform = 'translateX(4px)';
              }}
              onMouseLeave={e => {
                e.currentTarget.style.borderColor = 'var(--border)';
                e.currentTarget.style.transform = 'translateY(0)';
                e.currentTarget.style.boxShadow = 'none';
                const arrow = e.currentTarget.querySelector('.b404-arrow');
                if (arrow) arrow.style.transform = 'translateX(0)';
              }}
            >
              <div style={{
                display: 'flex', justifyContent: 'space-between', alignItems: 'center',
              }}>
                <span style={{
                  fontFamily: 'var(--font-sans)', fontSize: 11, fontWeight: 700,
                  textTransform: 'uppercase', letterSpacing: '0.12em',
                  color: 'var(--accent)',
                }}>{p.kicker}</span>
                <span className="b404-arrow" aria-hidden style={{
                  fontFamily: 'var(--font-sans)', fontSize: 18,
                  color: 'var(--accent)',
                  transition: 'transform 200ms var(--ease-out)',
                }}>→</span>
              </div>
              <div style={{
                fontFamily: 'var(--font-serif)',
                fontWeight: 600, fontStyle: 'italic',
                fontSize: 24, lineHeight: 1.15,
                letterSpacing: '-0.01em',
                color: 'var(--fg1)',
                marginTop: 4,
              }}>{p.title}</div>
              <div style={{
                fontFamily: 'var(--font-mono)', fontSize: 12,
                color: 'var(--fg3)',
                marginTop: 2,
              }}>{p.meta}</div>
            </a>
          ))}
        </div>
      </section>
    </main>
  );
};

// Big serif numeral — its own component so we keep the JSX above readable.
const Numeral = ({ children }) => (
  <span aria-hidden style={{
    fontFamily: 'var(--font-serif)',
    fontWeight: 600,
    fontStyle: 'italic',
    fontSize: 'clamp(180px, 24vw, 280px)',
    lineHeight: 0.9,
    letterSpacing: '-0.04em',
    color: 'var(--beava-brown-ink)',
    userSelect: 'none',
  }}>{children}</span>
);

window.Hero404 = Hero404;
