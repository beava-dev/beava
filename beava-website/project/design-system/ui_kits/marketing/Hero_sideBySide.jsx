// ui_kits/marketing/Hero.jsx
//
// Hero redesign — 2026-05-08, addressing review feedback:
//   1. Sharper headline (concrete job-to-be-done; no ambiguous "real-time features")
//   2. Single primary CTA (Run quickstart). Read docs is secondary; Discord moves to nav.
//   3. Right side recast as "Live feature queries" — events in → values out.
//      Each card reads like a request/response pair, not a metric dashboard tile.
//   4. Eyebrow does real work: Apache 2.0 · single binary · Python-defined streams.
//
// Tweaks panel exposes:
//   - headline   ('without-kafka' | 'compute-live' | 'counters-scores')
//   - cardStyle  ('query'  — function-call signature with → result
//                 'kv'     — key path with = result, more Redis-like
//                 'curl'   — full curl request + JSON response)
//   - showMascotPeek (boolean)

const HEADLINE_OPTIONS = {
  'heavy-infra': {
    text: 'Real-time user features without heavy streaming infra.',
    subhead: 'Define aggregations in Python, POST events over HTTP, and query fresh values by key in sub-millisecond latency. One binary. No Kafka, Flink, brokers, clusters, or stream jobs.',
  },
  'without-kafka': {
    text: 'Real-time user features without Kafka.',
    subhead: 'Define aggregations in Python, POST events over HTTP, and query fresh values by key in sub-millisecond latency. One binary. No brokers. No clusters. No stream jobs.',
  },
  'streaming-stack': {
    text: 'Real-time user features without the streaming stack.',
    subhead: 'Define aggregations in Python, POST events over HTTP, and query fresh values by key in sub-millisecond latency. One binary. No Kafka, Flink, brokers, clusters, or stream jobs.',
  },
};

// ─────────────────────────────────────────────────────────────────────────────
// Live data — real beava queries that update on a slow loop so the page feels
// alive without flickering. Keys are realistic (uid, path, window suffix).
// ─────────────────────────────────────────────────────────────────────────────

// beava primitives are namespaced under `bv.` — bv.count, bv.top_k, bv.last_seen, etc.
const seedQueries = () => ([
  {
    id: 'pv',
    fn: 'bv.count',
    args: [{ k: 'user_id', v: '"u_8412"' }, { k: 'window', v: '"24h"' }],
    keyExpr: 'bv.count:user:u_8412:24h',
    curl: { path: '/q/bv.count', body: '{"user_id":"u_8412","window":"24h"}', took: 0.6 },
    value: 74,
    fmt: (n) => Math.round(n).toLocaleString('en-US'),
    drift: () => 1 + Math.floor(Math.random() * 3),
  },
  {
    id: 'mt',
    fn: 'bv.p50',
    args: [{ k: 'path', v: '"/"' }, { k: 'window', v: '"1h"' }],
    keyExpr: 'bv.p50:path:/:1h',
    curl: { path: '/q/bv.p50', body: '{"path":"/","window":"1h"}', took: 0.4 },
    value: 1.2,
    unit: 's',
    fmt: (n) => `${n.toFixed(1)}s`,
    drift: () => (Math.random() - 0.5) * 0.18,
  },
  {
    id: 'tp',
    fn: 'bv.top_k',
    args: [{ k: 'metric', v: '"page_view"' }, { k: 'k', v: '1' }, { k: 'window', v: '"1h"' }],
    keyExpr: 'bv.top_k:page_view:1h',
    curl: { path: '/q/bv.top_k', body: '{"metric":"page_view","k":1,"window":"1h"}', took: 0.5 },
    value: { path: '"/"', count: 382 },
    rotateThru: [
      { path: '"/"',                       count: 382 },
      { path: '"/docs"',                   count: 264 },
      { path: '"/learn/chapter-1"',        count: 198 },
      { path: '"/docs/rolling-counters"',  count: 141 },
    ],
    fmt: (v) => `${v.path} · ${v.count.toLocaleString('en-US')}`,
  },
]);

const useLiveQueries = () => {
  const [rows, setRows] = React.useState(seedQueries);
  const [pulseId, setPulseId] = React.useState(null);
  React.useEffect(() => {
    let i = 0;
    const id = setInterval(() => {
      setRows(prev => prev.map((r, idx) => {
        // Each tick, only one row updates so the panel doesn't strobe.
        if (idx !== i % prev.length) return r;
        if (r.id === 'pv') return { ...r, value: r.value + r.drift() };
        if (r.id === 'mt') return { ...r, value: Math.max(0.4, r.value + r.drift()) };
        if (r.id === 'tp') {
          const list = r.rotateThru;
          const cur = list.findIndex(x => x.path === r.value.path);
          return { ...r, value: list[(cur + 1) % list.length] };
        }
        return r;
      }));
      setPulseId(rows[i % rows.length]?.id);
      i++;
      setTimeout(() => setPulseId(null), 700);
    }, 2400);
    return () => clearInterval(id);
  }, []); // eslint-disable-line
  return { rows, pulseId };
};

// ─────────────────────────────────────────────────────────────────────────────
// Card styles — three flavours, swappable via Tweaks
// ─────────────────────────────────────────────────────────────────────────────

const QueryCallSignature = ({ row }) => (
  <span style={{ fontFamily: 'var(--font-mono)', fontSize: 13.5, color: 'var(--fg2)', fontWeight: 500 }}>
    <span style={{ color: 'var(--code-fn)' }}>{row.fn}</span>
    <span style={{ color: 'var(--fg3)' }}>(</span>
    {row.args.map((a, i) => (
      <React.Fragment key={i}>
        {i > 0 && <span style={{ color: 'var(--fg3)' }}>, </span>}
        <span style={{ color: 'var(--code-keyword)' }}>{a.k}</span>
        <span style={{ color: 'var(--fg3)' }}>=</span>
        <span style={{ color: 'var(--code-string)' }}>{a.v}</span>
      </React.Fragment>
    ))}
    <span style={{ color: 'var(--fg3)' }}>)</span>
  </span>
);

const ResultPill = ({ row, pulse }) => {
  const text = row.fmt ? row.fmt(row.value) : String(row.value);
  return (
    <span
      key={text}
      style={{
        fontFamily: 'var(--font-mono)', fontSize: 15.5, fontWeight: 600,
        color: 'var(--fg1)',
        background: pulse ? 'var(--beava-orange-wash)' : 'transparent',
        padding: '2px 8px',
        borderRadius: 6,
        transition: 'background 600ms var(--ease-out)',
        whiteSpace: 'nowrap',
        animation: 'beava-fade-up 320ms var(--ease-out)',
      }}>
      {text}
    </span>
  );
};

const CardQuery = ({ row, pulse }) => (
  <div style={{
    background: '#fff', border: '1px solid var(--border)', borderRadius: 14,
    padding: '14px 18px', boxShadow: 'var(--shadow-sm)',
    display: 'flex', flexWrap: 'wrap',
    alignItems: 'center', columnGap: 12, rowGap: 6, minHeight: 56,
  }}>
    <div style={{ minWidth: 0, flex: '1 1 60%', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
      <QueryCallSignature row={row}/>
    </div>
    <span style={{ color: 'var(--accent)', fontFamily: 'var(--font-mono)', fontSize: 16, fontWeight: 600 }}>→</span>
    <ResultPill row={row} pulse={pulse}/>
  </div>
);

const CardKV = ({ row, pulse }) => {
  const text = row.fmt ? row.fmt(row.value) : String(row.value);
  return (
    <div style={{
      background: '#fff', border: '1px solid var(--border)', borderRadius: 14,
      padding: '14px 18px', boxShadow: 'var(--shadow-sm)',
      display: 'grid',
      gridTemplateColumns: 'minmax(0, 1fr) auto auto',
      alignItems: 'center', gap: 12, minHeight: 56,
    }}>
      <span style={{
        fontFamily: 'var(--font-mono)', fontSize: 13.5,
        color: 'var(--fg2)', fontWeight: 500, minWidth: 0,
        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
      }}>{row.keyExpr}</span>
      <span style={{ color: 'var(--fg3)', fontFamily: 'var(--font-mono)' }}>=</span>
      <span
        key={text}
        style={{
          fontFamily: 'var(--font-mono)', fontSize: 15.5, fontWeight: 600,
          color: 'var(--fg1)',
          background: pulse ? 'var(--beava-orange-wash)' : 'transparent',
          padding: '2px 8px', borderRadius: 6,
          transition: 'background 600ms var(--ease-out)',
          animation: 'beava-fade-up 320ms var(--ease-out)',
        }}>{text}</span>
    </div>
  );
};

const CardCurl = ({ row, pulse }) => {
  const text = row.fmt ? row.fmt(row.value) : String(row.value);
  const took = (row.curl.took + (Math.random() * 0.2)).toFixed(1);
  return (
    <div style={{
      background: '#fff', border: '1px solid var(--border)', borderRadius: 14,
      boxShadow: 'var(--shadow-sm)', overflow: 'hidden',
    }}>
      <div style={{
        background: 'var(--code-bg)',
        padding: '10px 14px',
        borderBottom: '1px solid var(--border)',
        fontFamily: 'var(--font-mono)', fontSize: 12.5,
        color: 'var(--fg2)', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
      }}>
        <span style={{ color: 'var(--accent)' }}>$ </span>
        <span style={{ color: 'var(--fg3)' }}>curl -sX POST</span>{' '}
        <span style={{ color: 'var(--code-string)' }}>localhost:6400{row.curl.path}</span>{' '}
        <span style={{ color: 'var(--fg3)' }}>-d</span>{' '}
        <span style={{ color: 'var(--code-string)' }}>'{row.curl.body}'</span>
      </div>
      <div style={{
        padding: '10px 14px', display: 'flex', alignItems: 'center', gap: 10,
      }}>
        <span style={{
          fontFamily: 'var(--font-mono)', fontSize: 11, fontWeight: 600,
          color: 'var(--beava-success)', background: 'var(--beava-success-wash)',
          padding: '2px 8px', borderRadius: 999, letterSpacing: 0,
        }}>200 · {took}ms</span>
        <span
          key={text}
          style={{
            fontFamily: 'var(--font-mono)', fontSize: 14.5, fontWeight: 600,
            color: 'var(--fg1)',
            background: pulse ? 'var(--beava-orange-wash)' : 'transparent',
            padding: '2px 8px', borderRadius: 6,
            transition: 'background 600ms var(--ease-out)',
            animation: 'beava-fade-up 320ms var(--ease-out)',
            flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
          }}>
          {`{ "value": ${typeof row.value === 'string' ? row.value : (row.fmt ? `"${row.fmt(row.value).replace(/"/g, '\\"')}"` : row.value)} }`}
        </span>
      </div>
    </div>
  );
};

const CARD_RENDERERS = { query: CardQuery, kv: CardKV, curl: CardCurl };

// ─────────────────────────────────────────────────────────────────────────────
// Right panel
// ─────────────────────────────────────────────────────────────────────────────

const LiveQueriesPanel = ({ cardStyle = 'query' }) => {
  const { rows, pulseId } = useLiveQueries();
  const Card = CARD_RENDERERS[cardStyle] || CardQuery;
  return (
    <div style={{ position: 'relative' }}>
      {/* Panel header */}
      <div style={{
        display: 'flex', alignItems: 'center', justifyContent: 'space-between',
        marginBottom: 14, paddingLeft: 4,
      }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <div style={{
            fontFamily: 'var(--font-sans)', fontSize: 12, fontWeight: 600,
            textTransform: 'uppercase', letterSpacing: '0.1em',
            color: 'var(--accent)',
          }}>
            Live feature queries
          </div>
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <span style={{
              width: 7, height: 7, borderRadius: 999, background: 'var(--beava-success)',
              boxShadow: '0 0 0 3px rgba(74,122,58,0.20)',
              animation: 'beava-pulse 2s var(--ease-in-out) infinite',
            }}/>
            <span style={{
              fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--fg3)',
              letterSpacing: 0,
            }}>auto-refresh</span>
          </span>
        </div>
        <div style={{
          fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--fg3)',
        }}>
          GET /q/...
        </div>
      </div>

      {/* Flow caption — events in → values out */}
      <div style={{
        display: 'grid', gridTemplateColumns: 'auto 1fr auto', alignItems: 'center', gap: 10,
        marginBottom: 14, padding: '0 4px',
      }}>
        <span style={{
          fontFamily: 'var(--font-mono)', fontSize: 11.5, color: 'var(--fg3)',
          padding: '3px 9px', background: 'var(--beava-paper)',
          border: '1px solid var(--border)', borderRadius: 999,
        }}>POST /push</span>
        <span style={{
          height: 1, background: `repeating-linear-gradient(90deg, var(--border-strong) 0 4px, transparent 4px 8px)`,
        }}/>
        <span style={{
          fontFamily: 'var(--font-mono)', fontSize: 11.5, color: 'var(--accent)',
          padding: '3px 9px', background: 'var(--beava-orange-wash)',
          border: '1px solid #f1d8c2', borderRadius: 999,
        }}>fresh values</span>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr', gap: 12 }}>
        {rows.map(r => <Card key={r.id} row={r} pulse={pulseId === r.id}/>)}
      </div>

      <p style={{
        margin: '14px 4px 0',
        fontFamily: 'var(--font-sans)', fontSize: 12.5, color: 'var(--fg3)',
        fontStyle: 'italic', lineHeight: 1.5,
      }}>
        Three live beava queries. Each is a function defined in Python, fed by HTTP events,
        queryable by key in <span style={{ color: 'var(--fg2)', fontStyle: 'normal' }}>&lt;1ms</span>.
        {' '}
        <a href="#pipeline" style={{
          color: 'var(--accent)', textDecoration: 'none',
          borderBottom: '1px solid color-mix(in oklab, var(--accent) 35%, transparent)',
          fontStyle: 'normal', fontWeight: 500,
        }}>See the pipeline ↓</a>
      </p>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// Install tabs (kept; this IS the quickstart, expanded)
// ─────────────────────────────────────────────────────────────────────────────

const INSTALL_TABS = [
  { id: 'brew',   label: 'brew',   cmd: 'brew install beava' },
  { id: 'curl',   label: 'curl',   cmd: 'curl -fsSL beava.dev/install.sh | sh' },
  { id: 'docker', label: 'docker', cmd: 'docker run -p 6400:6400 beava/beava' },
];

const InstallTabs = () => {
  const [tabId, setTabId] = React.useState('brew');
  const [copied, setCopied] = React.useState(false);
  const tab = INSTALL_TABS.find(t => t.id === tabId);

  const copy = () => {
    navigator.clipboard?.writeText(tab.cmd);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  return (
    <div style={{ marginBottom: 18 }}>
      <div style={{ display: 'flex', gap: 2, marginBottom: 0, paddingLeft: 4 }}>
        {INSTALL_TABS.map(t => {
          const active = t.id === tabId;
          return (
            <button
              key={t.id}
              onClick={() => setTabId(t.id)}
              style={{
                fontFamily: 'var(--font-mono)', fontSize: 12.5,
                padding: '6px 12px 8px',
                background: active ? 'var(--code-bg)' : 'transparent',
                color: active ? 'var(--fg1)' : 'var(--fg3)',
                border: '1px solid',
                borderColor: active ? 'var(--border)' : 'transparent',
                borderBottom: active ? '1px solid var(--code-bg)' : '1px solid var(--border)',
                borderRadius: '8px 8px 0 0',
                cursor: 'pointer', fontWeight: active ? 600 : 500,
                position: 'relative', top: 1,
                transition: 'color 200ms',
              }}>
              {t.label}
            </button>
          );
        })}
        <div style={{ flex: 1, borderBottom: '1px solid var(--border)' }}/>
      </div>
      <div style={{
        display: 'flex', alignItems: 'center', gap: 12,
        background: 'var(--code-bg)', border: '1px solid var(--border)', borderTop: 'none',
        borderRadius: '0 10px 10px 10px',
        padding: '12px 14px',
        boxShadow: 'var(--shadow-inset)',
      }}>
        <span style={{ color: 'var(--accent)', fontFamily: 'var(--font-mono)', fontSize: 14, userSelect: 'none' }}>$</span>
        <code style={{
          fontFamily: 'var(--font-mono)', fontSize: 14, color: 'var(--code-fg)',
          background: 'transparent', border: 0, padding: 0, flex: 1,
        }}>{tab.cmd}</code>
        <button onClick={copy} title="Copy" style={{
          fontFamily: 'var(--font-mono)', fontSize: 12,
          padding: '4px 10px',
          background: copied ? 'var(--beava-success-wash)' : '#fff',
          color: copied ? 'var(--beava-success)' : 'var(--fg2)',
          border: '1px solid',
          borderColor: copied ? '#cdd9b6' : 'var(--border)',
          borderRadius: 6, cursor: 'pointer', fontWeight: 600,
          display: 'inline-flex', alignItems: 'center', gap: 5,
          transition: 'all 200ms',
        }}>
          {copied ? <><Icon name="check" size={11}/> copied</> : <><Icon name="copy" size={11}/> copy</>}
        </button>
      </div>
    </div>
  );
};

// ─────────────────────────────────────────────────────────────────────────────
// Hero
// ─────────────────────────────────────────────────────────────────────────────

const HERO_DEFAULTS = /*EDITMODE-BEGIN*/{
  "headline": "heavy-infra",
  "cardStyle": "query",
  "showMascotPeek": true
}/*EDITMODE-END*/;

const Hero = () => {
  const [t, setTweak] = useTweaks(HERO_DEFAULTS);
  const headline = HEADLINE_OPTIONS[t.headline] || HEADLINE_OPTIONS['without-kafka'];

  return (
    <section style={{ padding: '56px 24px 96px', position: 'relative' }}>
      <div style={{
        maxWidth: 1200, margin: '0 auto',
        display: 'grid', gridTemplateColumns: '1.05fr 1fr', gap: 80, alignItems: 'center',
      }} className="beava-hero-grid">

        {/* LEFT — words */}
        <div>
          {/* Eyebrow does work now: license · binary · language */}
          <div style={{
            display: 'inline-flex', alignItems: 'center', gap: 10,
            padding: '5px 12px 5px 10px', borderRadius: 999,
            background: 'var(--beava-orange-wash)', border: '1px solid #f1d8c2',
            color: 'var(--accent)', fontSize: 12.5, fontWeight: 600,
            marginBottom: 22, fontFamily: 'var(--font-sans)',
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
            <span>Python-defined streams</span>
          </div>

          {/* Mascot wink */}
          <div style={{ marginBottom: 14 }}>
            <span style={{
              fontFamily: 'var(--font-accent)', fontWeight: 700,
              fontSize: 24, lineHeight: 1, color: 'var(--accent)',
              transform: 'rotate(-2deg)', transformOrigin: 'left center',
              display: 'inline-block',
            }}>
              Dam good at streams.
            </span>
          </div>

          {/* Headline — concrete, names the dread (Kafka) */}
          <h1 style={{
            fontFamily: 'var(--font-serif)', fontWeight: 600,
            fontSize: 'clamp(38px, 4.8vw, 60px)',
            lineHeight: 1.1, letterSpacing: '-0.02em',
            color: 'var(--fg1)', margin: '0 0 28px',
            maxWidth: 620, textWrap: 'balance',
          }}>
            {/* highlight the negation in italic+orange the way the system encourages */}
            {t.headline === 'heavy-infra' && <>Real-time user features <em style={{ color: 'var(--accent)', fontStyle: 'italic' }}>without heavy streaming infra.</em></>}
            {t.headline === 'without-kafka' && <>Real-time user features <em style={{ color: 'var(--accent)', fontStyle: 'italic' }}>without&nbsp;Kafka.</em></>}
            {t.headline === 'streaming-stack' && <>Real-time user features <em style={{ color: 'var(--accent)', fontStyle: 'italic' }}>without the streaming stack.</em></>}
          </h1>

          <p style={{
            fontFamily: 'var(--font-sans)', fontWeight: 400,
            fontSize: 17.5, lineHeight: 1.55,
            color: 'var(--fg2)', margin: '0 0 32px',
            maxWidth: 540, textWrap: 'pretty',
          }}>
            {headline.subhead}
          </p>

          <InstallTabs/>

          {/* Single primary; secondary as a text link to keep the focus on Run quickstart. */}
          <div style={{ display: 'flex', gap: 18, alignItems: 'center', marginTop: 20, flexWrap: 'wrap' }}>
            <Button variant="primary" size="lg" icon={<Icon name="arrow" size={16}/>}>
              Run quickstart
            </Button>
            <a href="#docs" style={{
              fontFamily: 'var(--font-sans)', fontSize: 14.5, fontWeight: 500,
              color: 'var(--fg2)', textDecoration: 'none',
              borderBottom: '1px solid color-mix(in oklab, var(--fg3) 35%, transparent)',
              paddingBottom: 1,
            }}>
              Read the docs →
            </a>
          </div>

          {/* Support line — replaces the "secondary CTA cluster" energy with a fact strip */}
          <div style={{
            marginTop: 22, paddingLeft: 4,
            fontFamily: 'var(--font-sans)', fontSize: 13, color: 'var(--fg3)',
            lineHeight: 1.5,
          }}>
            ~4 MB <span style={{ color: 'var(--border-strong)' }}>·</span> macOS, Linux, Windows
            <span style={{ color: 'var(--border-strong)' }}> · </span> runs on 1 GB RAM
            <span style={{ color: 'var(--border-strong)' }}> · </span> scales to one big box
          </div>
        </div>

        {/* RIGHT — live feature queries (was: dashboard) */}
        <div style={{ position: 'relative' }}>
          {t.showMascotPeek && (
            <img
              src="../../assets/mascot-pose-2.svg"
              alt=""
              width={108}
              height={108}
              style={{
                position: 'absolute',
                top: -82, right: -12,
                transform: 'rotate(8deg)',
                filter: 'drop-shadow(0 6px 14px rgba(26,23,20,0.12))',
                pointerEvents: 'none',
                zIndex: 2,
              }}
              className="beava-hero-mascot"
            />
          )}
          <LiveQueriesPanel cardStyle={t.cardStyle}/>
        </div>
      </div>

      {/* Tweaks */}
      <TweaksPanel>
        <TweakSection label="Headline"/>
        <TweakSelect
          label="Variant"
          value={t.headline}
          options={[
            { value: 'heavy-infra',     label: 'Without heavy streaming infra (default)' },
            { value: 'without-kafka',   label: 'Without Kafka (sharper)' },
            { value: 'streaming-stack', label: 'Without the streaming stack' },
          ]}
          onChange={(v) => setTweak('headline', v)}
        />
        <TweakSection label="Right panel"/>
        <TweakRadio
          label="Card style"
          value={t.cardStyle}
          options={['query', 'kv', 'curl']}
          onChange={(v) => setTweak('cardStyle', v)}
        />
        <TweakToggle
          label="Mascot peek"
          value={t.showMascotPeek}
          onChange={(v) => setTweak('showMascotPeek', v)}
        />
      </TweaksPanel>

      <style>{`
        @keyframes beava-pulse {
          0%, 100% { transform: scale(1); opacity: 1; }
          50%      { transform: scale(1.4); opacity: 0.55; }
        }
        @keyframes beava-fade-up {
          from { opacity: 0; transform: translateY(6px); }
          to   { opacity: 1; transform: translateY(0); }
        }
        @media (max-width: 960px) {
          .beava-hero-grid {
            grid-template-columns: 1fr !important;
            gap: 48px !important;
          }
          .beava-hero-mascot { display: none !important; }
        }
      `}</style>
    </section>
  );
};

window.Hero = Hero;
window.LiveQueriesPanel = LiveQueriesPanel;
