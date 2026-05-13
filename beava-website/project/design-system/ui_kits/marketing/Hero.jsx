// ui_kits/marketing/Hero.jsx
//
// Centered hero — 2026-05-12.
//   - One heading, one subhead, two CTAs, all centered.
//   - Live decision feed (event → fresh feature → decision) sits below the
//     fold-line as a single wide panel, not a side-by-side split.
//   - Repositioned around "AI products that react to live events".
//
// Tweaks expose:
//   - headlineEm   ('react'  | 'just-happened' | 'none')
//   - demoMode     ('auto'   — events tick in on a slow loop
//                   'send'   — auto + a row of send-test-event buttons)
//   - decisionTone ('orange' | 'green')
//   - showEyebrow  (boolean)

// ─────────────────────────────────────────────────────────────────────────────
// Decision feed — the demo that replaces the side panel
// ─────────────────────────────────────────────────────────────────────────────

const FEED_TEMPLATES = {
  login_failed: {
    event: 'login_failed',
    entity: () => `user_${1000 + Math.floor(Math.random() * 900)}`,
    feature: (n) => ({ key: 'failed_logins_10m', value: n }),
    valueOf: (prev) => (prev ? prev + 1 : 3 + Math.floor(Math.random() * 5)),
    decision: (v) => v >= 5 ? 'require verification' : 'increase risk score',
  },
  card_added: {
    event: 'card_added',
    entity: () => `device_${10 + Math.floor(Math.random() * 90)}`,
    feature: (n) => ({ key: 'cards_per_device_1h', value: n }),
    valueOf: (prev) => (prev ? prev + 1 : 2 + Math.floor(Math.random() * 3)),
    decision: (v) => v >= 4 ? 'flag for review' : 'increase risk score',
  },
  product_clicked: {
    event: 'product_clicked',
    entity: () => `user_${1000 + Math.floor(Math.random() * 900)}`,
    feature: () => ({ key: 'recent_clicks_30m', value: `${2 + Math.floor(Math.random() * 5)} items` }),
    decision: () => 'refresh recommendations',
  },
  llm_request: {
    event: 'llm_request',
    entity: () => `org_${['acme','globex','umbra','soylent'][Math.floor(Math.random()*4)]}`,
    feature: (n) => ({ key: 'tokens_used_24h', value: `${n}k` }),
    valueOf: (prev) => prev ? prev + 4 + Math.floor(Math.random()*8) : 60 + Math.floor(Math.random() * 40),
    decision: (v) => v >= 90 ? 'throttle expensive model' : 'route to cheap model',
  },
  payment_attempt: {
    event: 'payment_attempt',
    entity: () => `card_${10 + Math.floor(Math.random() * 90)}`,
    feature: (n) => ({ key: 'cvc_fails_1h', value: n }),
    valueOf: (prev) => prev ? prev + 1 : 1 + Math.floor(Math.random() * 3),
    decision: (v) => v >= 3 ? 'block transaction' : 'allow',
  },
  search_query: {
    event: 'search_query',
    entity: () => `user_${1000 + Math.floor(Math.random() * 900)}`,
    feature: () => ({ key: 'searches_5m', value: 4 + Math.floor(Math.random() * 10) }),
    decision: () => 'boost ranking signals',
  },
};

const ORDER = ['login_failed', 'card_added', 'product_clicked', 'llm_request', 'payment_attempt', 'search_query'];

const seed = () => {
  // Pre-fill with 4 rows so the feed isn't empty on first paint.
  const initial = ['login_failed', 'product_clicked', 'llm_request'];
  return initial.map((name, i) => mintRow(name, i, Date.now() - (initial.length - i) * 1800));
};

const mintRow = (name, idx, ts) => {
  const t = FEED_TEMPLATES[name];
  const entity = t.entity();
  const rawVal = t.valueOf ? t.valueOf() : null;
  const feat = t.feature(rawVal);
  const decision = t.decision(rawVal ?? feat.value);
  return {
    id: `${ts}-${idx}-${Math.random().toString(36).slice(2,6)}`,
    ts,
    event: t.event,
    entity,
    feature: feat,
    decision,
    age: 0,
    isNew: true,
  };
};

const useDecisionFeed = (auto = true) => {
  const [rows, setRows] = React.useState(seed);
  const [tickMs, setTickMs] = React.useState(0);

  // Append a new row
  const pushRow = React.useCallback((eventName) => {
    setRows(prev => {
      const ts = Date.now();
      const row = mintRow(eventName, prev.length, ts);
      const next = [row, ...prev.map(r => ({ ...r, isNew: false }))].slice(0, 3);
      return next;
    });
  }, []);

  // Auto-stream
  React.useEffect(() => {
    if (!auto) return;
    let i = 0;
    const id = setInterval(() => {
      const name = ORDER[Math.floor(Math.random() * ORDER.length)];
      pushRow(name);
      i++;
    }, 2400);
    return () => clearInterval(id);
  }, [auto, pushRow]);

  // Age ticker — updates "Xs ago" labels without rerendering the array
  React.useEffect(() => {
    const id = setInterval(() => setTickMs(Date.now()), 1000);
    return () => clearInterval(id);
  }, []);

  return { rows, pushRow, tickMs };
};

const ageLabel = (ts, now) => {
  const sec = Math.max(0, Math.floor((now - ts) / 1000));
  if (sec < 1) return 'just now';
  if (sec < 60) return `${sec}s ago`;
  return `${Math.floor(sec / 60)}m ago`;
};

// Column 1: event tag
const EventTag = ({ name }) => (
  <span style={{
    fontFamily: 'var(--font-mono)', fontSize: 12.5, fontWeight: 600,
    color: 'var(--fg2)',
    background: 'var(--beava-paper)',
    border: '1px solid var(--border)',
    borderRadius: 6, padding: '3px 8px',
    letterSpacing: 0, whiteSpace: 'nowrap',
  }}>{name}</span>
);

// Column 2: entity[feature] = value
const FeatureCell = ({ row }) => (
  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 13.5, color: 'var(--fg2)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>
    <span style={{ color: 'var(--fg3)' }}>{row.entity}</span>
    <span style={{ color: 'var(--fg3)' }}>.</span>
    <span style={{ color: 'var(--code-keyword)' }}>{row.feature.key}</span>
    <span style={{ color: 'var(--fg3)' }}> = </span>
    <span style={{ color: 'var(--fg1)', fontWeight: 600 }}>{row.feature.value}</span>
  </span>
);

// Column 3: decision
const DecisionCell = ({ text, tone }) => {
  const isGreen = tone === 'green';
  return (
    <span style={{
      display: 'inline-flex', alignItems: 'center', gap: 8,
      fontFamily: 'var(--font-sans)', fontSize: 14, fontWeight: 600,
      color: isGreen ? 'var(--beava-success)' : 'var(--accent)',
    }}>
      <span aria-hidden style={{ fontFamily: 'var(--font-mono)', opacity: 0.65 }}>→</span>
      {text}
    </span>
  );
};

const FeedRow = ({ row, now, tone, columnWidths }) => (
  <div
    style={{
      display: 'grid',
      gridTemplateColumns: columnWidths,
      alignItems: 'center', columnGap: 24,
      padding: '11px 22px',
      borderTop: '1px solid var(--border)',
      animation: row.isNew ? 'beava-row-in 420ms var(--ease-out)' : 'none',
      background: row.isNew ? 'var(--beava-orange-wash)' : 'transparent',
      transition: 'background 1200ms var(--ease-out)',
    }}
  >
    <div style={{ display: 'flex', alignItems: 'center', gap: 12, minWidth: 0 }}>
      <EventTag name={row.event}/>
      <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11.5, color: 'var(--fg3)', whiteSpace: 'nowrap' }}>
        {ageLabel(row.ts, now)}
      </span>
    </div>
    <div style={{ minWidth: 0 }}><FeatureCell row={row}/></div>
    <div style={{ minWidth: 0 }}><DecisionCell text={row.decision} tone={tone}/></div>
  </div>
);

const ColumnHeader = ({ children }) => (
  <div style={{
    fontFamily: 'var(--font-sans)', fontWeight: 600, fontSize: 11,
    textTransform: 'uppercase', letterSpacing: '0.1em',
    color: 'var(--fg3)',
  }}>{children}</div>
);

const SEND_BUTTONS = [
  { key: 'login_failed',    label: 'login_failed' },
  { key: 'product_clicked', label: 'product_clicked' },
  { key: 'llm_request',     label: 'llm_request' },
  { key: 'payment_attempt', label: 'payment_attempt' },
];

const DecisionFeed = ({ tone = 'orange', mode = 'auto', showHeaders = true }) => {
  const { rows, pushRow, tickMs } = useDecisionFeed(true);
  // 3-column grid, same template across header and body so columns align.
  const columnWidths = 'minmax(240px, 1.05fr) minmax(280px, 1.55fr) minmax(220px, 1.2fr)';

  return (
    <div style={{
      maxWidth: 1040, margin: '0 auto',
      background: '#fff',
      border: '1px solid var(--border)',
      borderRadius: 20,
      boxShadow: 'var(--shadow-md)',
      overflow: 'hidden',
    }}>
      {/* Top strip — title + live dot + endpoint */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        padding: '10px 22px',
        background: 'var(--beava-cream-deep)',
        borderBottom: '1px solid var(--border)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
          <span style={{
            fontFamily: 'var(--font-sans)', fontSize: 12, fontWeight: 700,
            color: 'var(--fg1)', textTransform: 'uppercase', letterSpacing: '0.12em',
          }}>Live decision feed</span>
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <span style={{
              width: 7, height: 7, borderRadius: 999, background: 'var(--beava-success)',
              boxShadow: '0 0 0 3px rgba(74,122,58,0.20)',
              animation: 'beava-pulse 2s var(--ease-in-out) infinite',
            }}/>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--beava-success)', fontWeight: 600 }}>live · connected</span>
          </span>
        </div>
        <a href="https://demo.beava.dev" target="_blank" rel="noopener" style={{
          display: 'inline-flex', alignItems: 'center', gap: 8,
          fontFamily: 'var(--font-mono)', fontSize: 11.5, color: 'var(--fg2)',
          textDecoration: 'none',
          padding: '3px 9px',
          background: '#fff',
          border: '1px solid var(--border)', borderRadius: 999,
          transition: 'all 160ms var(--ease-out)',
        }}
        onMouseEnter={e => { e.currentTarget.style.borderColor = 'var(--accent)'; e.currentTarget.style.color = 'var(--accent)'; }}
        onMouseLeave={e => { e.currentTarget.style.borderColor = 'var(--border)'; e.currentTarget.style.color = 'var(--fg2)'; }}
        >
          <span aria-hidden style={{ color: 'var(--fg3)' }}>›_</span>
          <span>demo.beava.dev</span>
          <span style={{ color: 'var(--fg3)' }}>↗</span>
        </a>
      </div>

      {/* Column headers */}
      {showHeaders && (
        <div style={{
          display: 'grid',
          gridTemplateColumns: columnWidths,
          columnGap: 24, padding: '8px 22px 6px',
        }}>
          <ColumnHeader>Event</ColumnHeader>
          <ColumnHeader>Fresh feature</ColumnHeader>
          <ColumnHeader>Decision</ColumnHeader>
        </div>
      )}

      {/* Rows */}
      <div>
        {rows.map((row) => (
          <FeedRow key={row.id} row={row} now={tickMs || Date.now()} tone={tone} columnWidths={columnWidths}/>
        ))}
      </div>

      {/* Footer — latency strip, and optional send-event buttons */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        gap: 16, flexWrap: 'wrap',
        padding: '9px 22px',
        borderTop: '1px solid var(--border)',
        background: 'var(--beava-paper)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 14, fontFamily: 'var(--font-mono)', fontSize: 11.5, color: 'var(--fg3)' }}>
          <span>fresh values</span>
          <span style={{ color: 'var(--border-strong)' }}>·</span>
          <span>updated <span style={{ color: 'var(--fg2)' }}>14ms ago</span></span>
          <span style={{ color: 'var(--border-strong)' }}>·</span>
          <span>no batch lag</span>
        </div>

        {mode === 'send' && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
            <span style={{ fontFamily: 'var(--font-sans)', fontStyle: 'italic', fontSize: 12.5, color: 'var(--fg3)' }}>
              send a test event:
            </span>
            {SEND_BUTTONS.map(b => (
              <button
                key={b.key}
                onClick={() => pushRow(b.key)}
                style={{
                  fontFamily: 'var(--font-mono)', fontSize: 11.5, fontWeight: 600,
                  background: '#fff', color: 'var(--fg1)',
                  border: '1px solid var(--border)', borderRadius: 999,
                  padding: '4px 10px', cursor: 'pointer',
                  transition: 'all 160ms var(--ease-out)',
                }}
                onMouseEnter={e => { e.currentTarget.style.borderColor = 'var(--accent)'; e.currentTarget.style.color = 'var(--accent)'; }}
                onMouseLeave={e => { e.currentTarget.style.borderColor = 'var(--border)'; e.currentTarget.style.color = 'var(--fg1)'; }}
              >
                {b.label}
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// Hero mascot — animated gif that plays once, then freezes on a canvas snap.
// 180 frames @ ~33ms ≈ 6s per loop. We snap at 5.9s so the freeze lands on
// the last frame before the loop restarts.
// ─────────────────────────────────────────────────────────────────────────────

const HeroMascot = () => {
  const imgRef = React.useRef(null);
  const canvasRef = React.useRef(null);
  const [frozen, setFrozen] = React.useState(false);

  React.useEffect(() => {
    const id = setTimeout(() => {
      const img = imgRef.current;
      const canvas = canvasRef.current;
      if (!img || !canvas || !img.complete) return;
      try {
        canvas.width = img.naturalWidth || 1000;
        canvas.height = img.naturalHeight || 1000;
        canvas.getContext('2d').drawImage(img, 0, 0, canvas.width, canvas.height);
        setFrozen(true);
      } catch (e) { /* tainted canvas — keep animating */ }
    }, 5900);
    return () => clearTimeout(id);
  }, []);

  const wrapStyle = {
    position: 'absolute',
    left: -18, top: -90,
    width: 130, height: 130,
    transform: 'rotate(-8deg)',
    filter: 'drop-shadow(0 12px 24px rgba(26,23,20,0.12))',
    pointerEvents: 'none', zIndex: 3,
  };

  return (
    <div className="beava-hero-mascot beava-hero-mascot--gif" style={wrapStyle}>
      <img
        ref={imgRef}
        src="../../assets/mascot-floating.gif"
        alt=""
        crossOrigin="anonymous"
        style={{
          position: 'absolute', inset: 0, width: '100%', height: '100%',
          opacity: frozen ? 0 : 1,
          transition: 'opacity 200ms var(--ease-out)',
        }}
      />
      <canvas
        ref={canvasRef}
        style={{
          position: 'absolute', inset: 0, width: '100%', height: '100%',
          opacity: frozen ? 1 : 0,
        }}
      />
    </div>
  );
};
window.HeroMascot = HeroMascot;

const HERO_DEFAULTS = /*EDITMODE-BEGIN*/{
  "headlineEm": "just-happened",
  "demoMode": "auto",
  "decisionTone": "orange",
  "showEyebrow": true,
  "showMascots": true
}/*EDITMODE-END*/;

const Hero = () => {
  const [t, setTweak] = useTweaks(HERO_DEFAULTS);

  // Italicize+orange one fragment of the heading per brand convention.
  const renderHeadline = () => {
    if (t.headlineEm === 'react') {
      return <>Make your AI product <em style={{ color: 'var(--accent)', fontStyle: 'italic' }}>react</em> to what just happened.</>;
    }
    if (t.headlineEm === 'just-happened') {
      return <>Make your AI product react to <em style={{ color: 'var(--accent)', fontStyle: 'italic' }}>what just happened.</em></>;
    }
    return <>Make your AI product react to what just happened.</>;
  };

  return (
    <section style={{ padding: '36px 24px 40px', position: 'relative', overflow: 'hidden' }}>
      <div style={{ maxWidth: 1200, margin: '0 auto', position: 'relative' }}>

        {/* Corner mascot — top-right of the hero section, slow-mo gif */}
        {t.showMascots && (
          <img
            src="../../assets/mascot-floating-slow.gif"
            alt=""
            className="beava-hero-mascot"
            style={{
              position: 'absolute',
              top: -8, right: 0,
              width: 140, height: 140,
              transform: 'rotate(8deg)',
              filter: 'drop-shadow(0 10px 22px rgba(26,23,20,0.12))',
              pointerEvents: 'none', userSelect: 'none',
              zIndex: 1,
            }}
          />
        )}

        {/* Floating mascot — moved next to demo card (see below) */}

        {/* Centered words */}
        <div style={{ textAlign: 'center', maxWidth: 820, margin: '0 auto 32px', position: 'relative', zIndex: 2 }}>
          {t.showEyebrow && (
            <div style={{
              display: 'inline-flex', alignItems: 'center', gap: 10,
              padding: '5px 12px 5px 10px', borderRadius: 999,
              background: 'var(--beava-orange-wash)', border: '1px solid #f1d8c2',
              color: 'var(--accent)', fontSize: 12, fontWeight: 600,
              marginBottom: 16, fontFamily: 'var(--font-sans)',
              whiteSpace: 'nowrap',
            }}>
              <span style={{
                width: 6, height: 6, background: 'var(--accent)', borderRadius: 999,
                boxShadow: '0 0 0 3px rgba(184,92,32,0.18)',
              }}/>
              <span>Apache 2.0</span>
              <span style={{ color: '#d8b594' }}>·</span>
              <span>single binary</span>
              <span style={{ color: '#d8b594' }}>·</span>
              <span>HTTP in, HTTP out</span>
            </div>
          )}

          <h1 style={{
            fontFamily: 'var(--font-serif)', fontWeight: 600,
            fontSize: 'clamp(28px, 3.4vw, 42px)',
            lineHeight: 1.18, letterSpacing: '-0.02em',
            color: 'var(--fg1)',
            margin: '0 auto 20px',
            maxWidth: '100%',
            textWrap: 'balance',
          }}>
            {renderHeadline()}
          </h1>

          <p style={{
            fontFamily: 'var(--font-sans)', fontWeight: 400,
            fontSize: 16.5, lineHeight: 1.55,
            color: 'var(--fg2)',
            margin: '0 auto 22px',
            maxWidth: 680,
            textWrap: 'pretty',
          }}>
            beava turns live events into fresh features for fraud, recommendations,
            and guardrails — no Kafka, no Flink, no feature store.
          </p>

          <div style={{ display: 'inline-flex', gap: 14, alignItems: 'center', flexWrap: 'wrap', justifyContent: 'center' }}>
            <Button variant="primary" size="md" icon={<Icon name="arrow" size={14}/>}>
              Build a fraud feature
            </Button>
            <a href="#docs" style={{
              fontFamily: 'var(--font-sans)', fontSize: 15, fontWeight: 500,
              color: 'var(--fg2)', textDecoration: 'none',
              borderBottom: '1px solid color-mix(in oklab, var(--fg3) 35%, transparent)',
              paddingBottom: 1,
            }}>
              Read the docs →
            </a>
          </div>

          <div style={{
            marginTop: 12,
            fontFamily: 'var(--font-sans)', fontSize: 12.5, color: 'var(--fg3)',
            lineHeight: 1.5,
          }}>
            Build your first live feature in minutes with beava's LLM-ready SDK docs.
          </div>
        </div>

        {/* Demo, centered below */}
        <div style={{ position: 'relative', maxWidth: 1040, margin: '0 auto' }}>
          <DecisionFeed tone={t.decisionTone} mode={t.demoMode} showHeaders={true}/>
        </div>

        {/* Tiny caption under demo */}
        <p style={{
          marginTop: 12,
          textAlign: 'center',
          fontFamily: 'var(--font-sans)', fontSize: 13, color: 'var(--fg3)',
          fontStyle: 'italic',
        }}>
          Real events streaming from a public beava instance at{' '}
          <a href="https://demo.beava.dev" target="_blank" rel="noopener" style={{
            color: 'var(--accent)', textDecoration: 'none', fontFamily: 'var(--font-mono)',
            fontStyle: 'normal', fontWeight: 500,
            borderBottom: '1px solid color-mix(in oklab, var(--accent) 35%, transparent)',
          }}>demo.beava.dev</a>
          {' '}— not a mock.{' '}
          <a href="#pipeline" style={{
            color: 'var(--accent)', textDecoration: 'none',
            borderBottom: '1px solid color-mix(in oklab, var(--accent) 35%, transparent)',
            fontStyle: 'normal', fontWeight: 500,
          }}>See the pipeline ↓</a>
        </p>
      </div>

      {/* Tweaks */}
      <TweaksPanel>
        <TweakSection label="Headline"/>
        <TweakRadio
          label="Italicize"
          value={t.headlineEm}
          options={['react', 'just-happened', 'none']}
          onChange={(v) => setTweak('headlineEm', v)}
        />
        <TweakToggle
          label="Show eyebrow pill"
          value={t.showEyebrow}
          onChange={(v) => setTweak('showEyebrow', v)}
        />
        <TweakToggle
          label="Show mascots"
          value={t.showMascots}
          onChange={(v) => setTweak('showMascots', v)}
        />
        <TweakSection label="Demo"/>
        <TweakRadio
          label="Mode"
          value={t.demoMode}
          options={['auto', 'send']}
          onChange={(v) => setTweak('demoMode', v)}
        />
        <TweakRadio
          label="Decision tone"
          value={t.decisionTone}
          options={['orange', 'green']}
          onChange={(v) => setTweak('decisionTone', v)}
        />
      </TweaksPanel>

      <style>{`
        @keyframes beava-pulse {
          0%, 100% { transform: scale(1); opacity: 1; }
          50%      { transform: scale(1.4); opacity: 0.55; }
        }
        @keyframes beava-row-in {
          from { opacity: 0; transform: translateY(-6px); }
          to   { opacity: 1; transform: translateY(0); }
        }
        @media (max-width: 1100px) {
          .beava-hero-mascot { display: none !important; }
        }
      `}</style>
    </section>
  );
};

window.Hero = Hero;
window.DecisionFeed = DecisionFeed;
