// ui_kits/marketing/Pipelines.jsx
//
// "Three reflex pipelines. Six signals your product can act on instantly."
//
// LEFT: big vertical tab list — one per pipeline, each with a big heading
//       (motivation) and a subhead.
// RIGHT: the actual beava SDK code for that pipeline (Python), styled like
//        PipelineShowcase, plus a signals strip beneath it.
//
// Auto-rotates every 8 seconds. Clicking a tab resets the timer so the
// user's choice stays put for the full interval before rotation resumes.

const ROTATE_MS = 8000;

// Re-usable code token styles (match PipelineShowcase.jsx)
const CS = {
  kw:  { color: 'var(--code-keyword)' },
  str: { color: 'var(--code-string)' },
  cmt: { color: 'var(--code-comment)', fontStyle: 'italic' },
  fn:  { color: 'var(--code-fn)' },
  num: { color: 'var(--code-number)' },
  ty:  { color: 'var(--code-type)' },
};

// Highlight colors for feature names (and matching signal dot)
const HL = {
  orange: 'rgba(184,92,32,0.10)',
  blue:   'rgba(58,106,138,0.10)',
  amber:  'rgba(217,122,62,0.12)',
};
const DOT = {
  orange: 'var(--accent)',
  blue:   'var(--beava-info)',
  amber:  'var(--beava-orange-soft)',
};

// ─────────────────────────────────────────────────────────────────────────────
// Code blocks — one per pipeline.
// Returned as a render function so token styles + highlights stay inline.
// ─────────────────────────────────────────────────────────────────────────────

const AgentCode = () => (
  <>
    <span style={CS.kw}>import</span> beava <span style={CS.kw}>as</span> bv{'\n'}
    {'\n'}
    <span style={CS.fn}>@bv.event</span>{'\n'}
    <span style={CS.kw}>class</span> <span style={CS.ty}>AgentStep</span>:{'\n'}
    {'    '}agent_id: <span style={CS.ty}>str</span>{'\n'}
    {'    '}session_id: <span style={CS.ty}>str</span>{'\n'}
    {'    '}tool: <span style={CS.ty}>str</span>     <span style={CS.cmt}># "shell_exec" · "http_get" · "code_run"</span>{'\n'}
    {'    '}risky: <span style={CS.ty}>bool</span>{'\n'}
    {'\n'}
    <span style={CS.fn}>@bv.table</span>(key=<span style={CS.str}>"agent_id"</span>){'\n'}
    <span style={CS.kw}>def</span> <span style={CS.fn}>SessionReflexes</span>(e: <span style={CS.ty}>AgentStep</span>):{'\n'}
    {'    '}<span style={CS.kw}>return</span> e.<span style={CS.fn}>agg</span>({'\n'}
    {'        '}<span style={{ background: HL.orange, borderRadius: 3, padding: '0 2px' }}>steps_30s</span>        = bv.<span style={CS.fn}>count</span>(window=<span style={CS.str}>"30s"</span>),{'\n'}
    {'        '}<span style={{ background: HL.blue, borderRadius: 3, padding: '0 2px' }}>risky_tools_10m</span>  = bv.<span style={CS.fn}>count</span>(window=<span style={CS.str}>"10m"</span>, where=<span style={CS.str}>"_event.risky"</span>),{'\n'}
    {'    '}){'\n'}
    {'\n'}
    bv.<span style={CS.fn}>App</span>(<span style={CS.str}>"0.0.0.0:6400"</span>).<span style={CS.fn}>register</span>(<span style={CS.ty}>AgentStep</span>, <span style={CS.fn}>SessionReflexes</span>).<span style={CS.fn}>serve</span>()
  </>
);

const MarketplaceCode = () => (
  <>
    <span style={CS.kw}>import</span> beava <span style={CS.kw}>as</span> bv{'\n'}
    {'\n'}
    <span style={CS.fn}>@bv.event</span>{'\n'}
    <span style={CS.kw}>class</span> <span style={CS.ty}>CommerceEvent</span>:{'\n'}
    {'    '}user_id: <span style={CS.ty}>str</span>{'\n'}
    {'    '}sku: <span style={CS.ty}>str</span>{'\n'}
    {'    '}kind: <span style={CS.ty}>str</span>      <span style={CS.cmt}># "view" · "add_to_cart" · "purchase"</span>{'\n'}
    {'    '}price: <span style={CS.ty}>float</span>{'\n'}
    {'\n'}
    <span style={CS.fn}>@bv.table</span>(key=<span style={CS.str}>"sku"</span>){'\n'}
    <span style={CS.kw}>def</span> <span style={CS.fn}>SkuMomentum</span>(e: <span style={CS.ty}>CommerceEvent</span>):{'\n'}
    {'    '}<span style={CS.kw}>return</span> e.<span style={CS.fn}>agg</span>({'\n'}
    {'        '}<span style={{ background: HL.orange, borderRadius: 3, padding: '0 2px' }}>carts_5m</span> = bv.<span style={CS.fn}>count</span>(window=<span style={CS.str}>"5m"</span>, where=<span style={CS.str}>"_event.kind == 'add_to_cart'"</span>),{'\n'}
    {'    '}){'\n'}
    {'\n'}
    <span style={CS.fn}>@bv.table</span>(key=<span style={CS.str}>"user_id"</span>){'\n'}
    <span style={CS.kw}>def</span> <span style={CS.fn}>ShopperReflexes</span>(e: <span style={CS.ty}>CommerceEvent</span>):{'\n'}
    {'    '}<span style={CS.kw}>return</span> e.<span style={CS.fn}>agg</span>({'\n'}
    {'        '}<span style={{ background: HL.blue, borderRadius: 3, padding: '0 2px' }}>avg_view_price_30m</span> = bv.<span style={CS.fn}>avg</span>(e.price, window=<span style={CS.str}>"30m"</span>,{'\n'}
    {'                                       '}where=<span style={CS.str}>"_event.kind == 'view'"</span>),{'\n'}
    {'    '}){'\n'}
    {'\n'}
    bv.<span style={CS.fn}>App</span>(<span style={CS.str}>"0.0.0.0:6400"</span>).<span style={CS.fn}>register</span>(<span style={CS.ty}>CommerceEvent</span>, <span style={CS.fn}>SkuMomentum</span>, <span style={CS.fn}>ShopperReflexes</span>).<span style={CS.fn}>serve</span>()
  </>
);

const SaaSCode = () => (
  <>
    <span style={CS.kw}>import</span> beava <span style={CS.kw}>as</span> bv{'\n'}
    {'\n'}
    <span style={CS.fn}>@bv.event</span>{'\n'}
    <span style={CS.kw}>class</span> <span style={CS.ty}>ProductEvent</span>:{'\n'}
    {'    '}user_id: <span style={CS.ty}>str</span>{'\n'}
    {'    '}org_id: <span style={CS.ty}>str</span>{'\n'}
    {'    '}kind: <span style={CS.ty}>str</span>      <span style={CS.cmt}># "error" · "limit_hit" · "feature_used"</span>{'\n'}
    {'    '}topic: <span style={CS.ty}>str</span>{'\n'}
    {'\n'}
    <span style={CS.fn}>@bv.table</span>(key=<span style={CS.str}>"user_id"</span>){'\n'}
    <span style={CS.kw}>def</span> <span style={CS.fn}>UserActivation</span>(e: <span style={CS.ty}>ProductEvent</span>):{'\n'}
    {'    '}<span style={CS.kw}>return</span> e.<span style={CS.fn}>agg</span>({'\n'}
    {'        '}<span style={{ background: HL.orange, borderRadius: 3, padding: '0 2px' }}>errors_10m</span> = bv.<span style={CS.fn}>count</span>(window=<span style={CS.str}>"10m"</span>, by=e.topic,{'\n'}
    {'                          '}where=<span style={CS.str}>"_event.kind == 'error'"</span>),{'\n'}
    {'    '}){'\n'}
    {'\n'}
    <span style={CS.fn}>@bv.table</span>(key=<span style={CS.str}>"org_id"</span>){'\n'}
    <span style={CS.kw}>def</span> <span style={CS.fn}>OrgExpansionSignals</span>(e: <span style={CS.ty}>ProductEvent</span>):{'\n'}
    {'    '}<span style={CS.kw}>return</span> e.<span style={CS.fn}>agg</span>({'\n'}
    {'        '}<span style={{ background: HL.blue, borderRadius: 3, padding: '0 2px' }}>limit_hits_24h</span> = bv.<span style={CS.fn}>count</span>(window=<span style={CS.str}>"24h"</span>,{'\n'}
    {'                              '}where=<span style={CS.str}>"_event.kind == 'limit_hit'"</span>),{'\n'}
    {'    '}){'\n'}
    {'\n'}
    bv.<span style={CS.fn}>App</span>(<span style={CS.str}>"0.0.0.0:6400"</span>).<span style={CS.fn}>register</span>(<span style={CS.ty}>ProductEvent</span>, <span style={CS.fn}>UserActivation</span>, <span style={CS.fn}>OrgExpansionSignals</span>).<span style={CS.fn}>serve</span>()
  </>
);

// ─────────────────────────────────────────────────────────────────────────────
// Pipelines model
// ─────────────────────────────────────────────────────────────────────────────
const PIPELINES = [
  {
    id: 'agent',
    num: '01',
    kicker: 'Agent runtime control',
    heading: 'Pause runaway agents before they burn money.',
    sub: 'Stop runaway agents before they burn money or touch the wrong tool.',
    filename: 'agent_safety.py',
    lines: '15 lines',
    Code: AgentCode,
    signals: [
      { tone: 'orange', entity: 'agent_91',   feature: 'steps_30s',       value: '8',  threshold: '> 6',  action: 'pause runaway agent',      gain: '+ $84 saved' },
      { tone: 'blue',   entity: 'session_44', feature: 'risky_tools_10m', value: '2',  threshold: '≥ 2',  action: 'require human approval',   gain: '+ $250 risk avoided' },
    ],
  },
  {
    id: 'marketplace',
    num: '02',
    kicker: 'Marketplace reranking',
    heading: 'Reorder the marketplace while shoppers are still shopping.',
    sub: 'Boost trending items and sort toward live price intent — without a nightly batch.',
    filename: 'marketplace_ranking.py',
    lines: '18 lines',
    Code: MarketplaceCode,
    signals: [
      { tone: 'orange', entity: 'sku_882',  feature: 'carts_5m',           value: '91',   threshold: '> 50',   action: 'boost trending item',         gain: '+ $1.40 GMV' },
      { tone: 'blue',   entity: 'user_2041', feature: 'avg_view_price_30m', value: '$240', threshold: '> $200', action: 'sort toward premium picks',   gain: '+ $3.80 GMV' },
    ],
  },
  {
    id: 'saas',
    num: '03',
    kicker: 'SaaS growth rescue',
    heading: 'Rescue stuck users while the window is still open.',
    sub: 'Surface setup help and expansion paths while users are active — not in tomorrow\u2019s email.',
    filename: 'growth_rescue.py',
    lines: '18 lines',
    Code: SaaSCode,
    signals: [
      { tone: 'orange', entity: 'user_2041', feature: 'errors_10m',     value: '6',  threshold: '> 5',  meta: 'topic="auth"', action: 'launch setup rescue',     gain: '+ $29 LTV' },
      { tone: 'blue',   entity: 'org_acme',  feature: 'limit_hits_24h', value: '12', threshold: '> 10',                       action: 'show team upgrade path',  gain: '+ $99 MRR' },
    ],
  },
];

// ─────────────────────────────────────────────────────────────────────────────
// LEFT — Big vertical tab
// ─────────────────────────────────────────────────────────────────────────────
const PipelineTab = ({ p, active, onClick, rotateMs }) => {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-pressed={active}
      style={{
        textAlign: 'left',
        display: 'block', width: '100%',
        cursor: 'pointer',
        padding: '26px 26px 24px',
        borderRadius: 16,
        border: '1px solid ' + (active ? 'var(--accent)' : 'var(--border)'),
        background: active ? '#fff' : 'var(--beava-paper)',
        boxShadow: active
          ? '0 1px 2px rgba(26,23,20,0.05), 0 8px 24px rgba(184,92,32,0.12)'
          : '0 1px 2px rgba(26,23,20,0.03)',
        position: 'relative', overflow: 'hidden',
        transition: 'all 200ms cubic-bezier(0.22,1,0.36,1)',
        outline: 'none',
        font: 'inherit', color: 'inherit',
      }}
      onMouseEnter={e => {
        if (active) return;
        e.currentTarget.style.background = '#fff';
        e.currentTarget.style.borderColor = 'var(--border-strong)';
      }}
      onMouseLeave={e => {
        if (active) return;
        e.currentTarget.style.background = 'var(--beava-paper)';
        e.currentTarget.style.borderColor = 'var(--border)';
      }}
    >
      {/* Active indicator — left rail */}
      <span aria-hidden style={{
        position: 'absolute', left: 0, top: 18, bottom: 18, width: 3,
        borderRadius: 3,
        background: active ? 'var(--accent)' : 'transparent',
        transition: 'background 200ms var(--ease-out)',
      }}/>

      <div style={{
        display: 'flex', alignItems: 'baseline', gap: 12, marginBottom: 10,
      }}>
        <span style={{
          fontFamily: 'var(--font-mono)', fontSize: 12, fontWeight: 600,
          color: active ? 'var(--accent)' : 'var(--fg3)',
          letterSpacing: '0.04em',
        }}>{p.num}</span>
        <span style={{
          fontFamily: 'var(--font-sans)', fontSize: 11, fontWeight: 600,
          textTransform: 'uppercase', letterSpacing: '0.08em',
          color: active ? 'var(--accent)' : 'var(--fg3)',
        }}>{p.kicker}</span>
      </div>

      <h3 style={{
        fontFamily: 'var(--font-serif)', fontWeight: 600,
        fontSize: 26, lineHeight: 1.18, letterSpacing: '-0.015em',
        color: 'var(--fg1)', margin: '0 0 10px',
        textWrap: 'balance',
      }}>{p.heading}</h3>

      <p style={{
        fontFamily: 'var(--font-sans)', fontSize: 15, lineHeight: 1.55,
        color: 'var(--fg2)', margin: 0, textWrap: 'pretty',
      }}>{p.sub}</p>

      {/* Progress underline — fills over rotateMs while this tab is active */}
      {active && (
        <span aria-hidden style={{
          position: 'absolute', left: 0, bottom: 0, height: 2,
          background: 'var(--accent)',
          animation: `beava-tab-progress ${rotateMs}ms linear forwards`,
        }}/>
      )}
    </button>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// RIGHT — Code SDK card (file chrome + code + signals)
// ─────────────────────────────────────────────────────────────────────────────
// Outcome card — exactly two lines:
//   Line 1 — entity.feature = value · crossed > threshold (mono caption)
//   Line 2 — action (serif, orange)
const OutcomeCard = ({ s, i, total }) => {
  return (
    <div style={{
      position: 'relative',
      padding: '18px 22px 20px 26px',
      borderRight: i < total - 1 ? '1px solid var(--border-inset)' : 'none',
      background: '#fff',
      display: 'flex', flexDirection: 'column', gap: 8,
      minWidth: 0,
    }}>
      {/* Left accent rail in the tone color */}
      <span aria-hidden style={{
        position: 'absolute', left: 0, top: 16, bottom: 16, width: 3,
        borderRadius: 3, background: DOT[s.tone],
      }}/>

      {/* Line 1 — signal read: entity.feature = value > threshold */}
      <div style={{
        fontFamily: 'var(--font-mono)', fontSize: 12.5, lineHeight: 1.45,
        color: 'var(--fg3)',
        overflowWrap: 'anywhere',
      }}>
        <span>{s.entity}.</span>
        <span style={{
          background: HL[s.tone], padding: '1px 5px', borderRadius: 4,
          color: 'var(--code-fg)', fontWeight: 600,
        }}>{s.feature}</span>
        <span> = </span>
        <span style={{ color: 'var(--fg1)', fontWeight: 700 }}>{s.value}</span>
        <span style={{ color: 'var(--accent)', fontWeight: 600 }}>{' '}{s.threshold}</span>
        {s.meta && (
          <span>{'  ·  '}{s.meta}</span>
        )}
      </div>

      {/* Line 2 — action (the outcome) + dollar gain marker */}
      <div style={{
        display: 'flex', alignItems: 'baseline', gap: 10, flexWrap: 'wrap',
      }}>
        <span style={{
          fontFamily: 'var(--font-serif)', fontWeight: 600,
          fontSize: 20, lineHeight: 1.2, letterSpacing: '-0.015em',
          color: 'var(--accent)',
          textWrap: 'balance',
        }}>
          {s.action}
        </span>
        {s.gain && (
          <span style={{
            fontFamily: 'var(--font-accent)', fontWeight: 700, fontSize: 18,
            color: 'var(--beava-success)', lineHeight: 1,
            transform: 'rotate(-3deg)', display: 'inline-block',
            whiteSpace: 'nowrap',
          }}>{s.gain}</span>
        )}
      </div>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// RIGHT — Outcomes (top, prominent) + Code SDK (below)
// ─────────────────────────────────────────────────────────────────────────────
const PipelineCodePanel = ({ p }) => {
  const Code = p.Code;
  return (
    <div
      key={p.id}
      style={{
        background: '#fff', border: '1px solid var(--border)', borderRadius: 16,
        boxShadow: '0 1px 2px rgba(26,23,20,0.04), 0 8px 24px rgba(26,23,20,0.06)',
        overflow: 'hidden',
        display: 'flex', flexDirection: 'column',
        height: '100%',
        animation: 'beava-fade-in 320ms var(--ease-out)',
      }}
    >
      {/* === OUTCOMES — top, the headline of this panel === */}
      <div style={{
        padding: '12px 22px 10px',
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        gap: 12, flexWrap: 'wrap',
        background: '#fff',
        borderBottom: '1px solid var(--border-inset)',
      }}>
        <span style={{
          fontFamily: 'var(--font-sans)', fontWeight: 700, fontSize: 10.5,
          textTransform: 'uppercase', letterSpacing: '0.14em',
          color: 'var(--fg3)',
        }}>Live signals → actions</span>
      </div>
      <div style={{
        display: 'grid',
        gridTemplateColumns: `repeat(${p.signals.length}, minmax(0, 1fr))`,
      }}>
        {p.signals.map((s, i) => (
          <OutcomeCard key={i} s={s} i={i} total={p.signals.length}/>
        ))}
      </div>

      {/* === CODE — the how, below === */}
      {/* File chrome */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        gap: 12, flexWrap: 'wrap',
        padding: '10px 16px',
        borderTop: '1px solid var(--border)',
        borderBottom: '1px solid var(--border)',
        background: 'var(--beava-paper)',
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, minWidth: 0 }}>
          <span style={{ width: 9, height: 9, borderRadius: 999, background: '#e6dccb' }}/>
          <span style={{ width: 9, height: 9, borderRadius: 999, background: '#e6dccb' }}/>
          <span style={{ width: 9, height: 9, borderRadius: 999, background: '#e6dccb' }}/>
          <span style={{
            fontFamily: 'var(--font-mono)', fontSize: 12.5, color: 'var(--fg2)',
            marginLeft: 6, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
          }}>
            {p.filename}
          </span>
          <span style={{
            fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--fg3)',
            padding: '2px 8px', borderRadius: 999,
            background: '#fff', border: '1px solid var(--border)', marginLeft: 4,
            whiteSpace: 'nowrap',
          }}>{p.lines}</span>
        </div>
        <span style={{
          display: 'inline-flex', alignItems: 'center', gap: 6,
          fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--beava-success)',
          whiteSpace: 'nowrap', fontWeight: 600,
        }}>
          <span style={{ width: 6, height: 6, borderRadius: 999, background: 'var(--beava-success)' }}/>
          registered · live &lt;1ms
        </span>
      </div>

      {/* Code body */}
      <pre
        style={{
          margin: 0, padding: '20px 24px',
          fontFamily: 'var(--font-mono)', fontSize: 13.5, lineHeight: 1.65,
          color: 'var(--code-fg)', background: 'var(--code-bg)',
          border: 0, borderRadius: 0, boxShadow: 'none', overflowX: 'auto',
          flex: 1,
        }}
      >
        <Code/>
      </pre>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// Section
// ─────────────────────────────────────────────────────────────────────────────
const Pipelines = () => {
  const [idx, setIdx] = React.useState(0);
  // Increments whenever the user clicks — used to reset the rotation timer.
  const [resetTick, setResetTick] = React.useState(0);

  // Auto-rotate every ROTATE_MS. Resets on resetTick or idx change.
  React.useEffect(() => {
    const id = setTimeout(() => {
      setIdx((i) => (i + 1) % PIPELINES.length);
    }, ROTATE_MS);
    return () => clearTimeout(id);
  }, [idx, resetTick]);

  const active = PIPELINES[idx];

  const pickTab = (i) => {
    setIdx(i);
    setResetTick((t) => t + 1);
  };

  return (
    <section id="pipelines" style={{
      padding: '96px 24px 104px',
      background: 'var(--bg-alt)',
      borderTop: '1px solid var(--border)',
      borderBottom: '1px solid var(--border)',
    }}>
      <div style={{ maxWidth: 1200, margin: '0 auto' }}>
        {/* Section header */}
        <div style={{ marginBottom: 48, maxWidth: 760 }}>
          <Eyebrow>Three pipelines. Six live signals.</Eyebrow>
          <h2 style={{
            fontFamily: 'var(--font-serif)', fontWeight: 600,
            fontSize: 'clamp(32px, 3.6vw, 46px)',
            lineHeight: 1.1, letterSpacing: '-0.02em',
            color: 'var(--fg1)', margin: '12px 0 14px',
            textWrap: 'balance',
          }}>
            Three reflex pipelines. Six signals your product can{' '}
            <em style={{ color: 'var(--accent)', fontStyle: 'italic' }}>act on instantly.</em>
          </h2>
          <p style={{
            fontFamily: 'var(--font-sans)', fontSize: 17, lineHeight: 1.55,
            color: 'var(--fg2)', margin: 0, maxWidth: 640, textWrap: 'pretty',
          }}>
            Beava turns live events into fresh decision features, so your app can pause
            runaway agents, reorder marketplaces, and rescue stuck users — no Kafka, no
            Flink, no feature store.
          </p>
        </div>

        {/* Tabs + code */}
        <div className="beava-pipelines-grid" style={{
          display: 'grid',
          gridTemplateColumns: 'minmax(340px, 0.95fr) minmax(0, 1.25fr)',
          gap: 24,
          alignItems: 'stretch',
        }}>
          {/* LEFT — big vertical tabs */}
          <div
            role="tablist"
            aria-label="Reflex pipelines"
            style={{ display: 'flex', flexDirection: 'column', gap: 14 }}
          >
            {PIPELINES.map((p, i) => (
              <PipelineTab
                key={p.id + '-' + idx + '-' + resetTick + '-' + i}
                p={p}
                active={i === idx}
                onClick={() => pickTab(i)}
                rotateMs={ROTATE_MS}
              />
            ))}
          </div>

          {/* RIGHT — code SDK + signals */}
          <div role="tabpanel">
            <PipelineCodePanel p={active}/>
          </div>
        </div>
      </div>

      <style>{`
        @keyframes beava-tab-progress {
          from { width: 0%; }
          to   { width: 100%; }
        }
        @keyframes beava-fade-in {
          from { opacity: 0; transform: translateY(2px); }
          to   { opacity: 1; transform: translateY(0); }
        }
        @media (max-width: 920px) {
          .beava-pipelines-grid {
            grid-template-columns: 1fr !important;
          }
        }
      `}</style>
    </section>
  );
};
window.Pipelines = Pipelines;
