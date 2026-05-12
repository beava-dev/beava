// ui_kits/marketing/Hero.jsx
// Hero redesign — locked decisions 2026-05-04.
// Left: words. Right: LiveMetrics dashboard panel (replaces FeedBeaver).

const useTickingNumber = (target, { duration = 1100, decimals = 0 } = {}) => {
  const [val, setVal] = React.useState(target);
  const fromRef = React.useRef(target);
  React.useEffect(() => {
    const from = fromRef.current;
    const to = target;
    if (from === to) return;
    const start = performance.now();
    let raf;
    const step = (now) => {
      const t = Math.min(1, (now - start) / duration);
      const eased = 1 - Math.pow(1 - t, 3);
      const v = from + (to - from) * eased;
      setVal(v);
      if (t < 1) raf = requestAnimationFrame(step);
      else fromRef.current = to;
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  }, [target, duration]);
  return decimals === 0 ? Math.round(val) : Number(val.toFixed(decimals));
};

const Sparkline = ({ data, color = 'var(--accent)', w = 140, h = 36 }) => {
  if (!data || data.length < 2) return null;
  const min = Math.min(...data);
  const max = Math.max(...data);
  const span = max - min || 1;
  const stepX = w / (data.length - 1);
  const pts = data.map((v, i) => [i * stepX, h - ((v - min) / span) * (h - 4) - 2]);
  const d = pts.map(([x, y], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${y.toFixed(1)}`).join(' ');
  const area = `${d} L${w},${h} L0,${h} Z`;
  const last = pts[pts.length - 1];
  return (
    <svg width={w} height={h} viewBox={`0 0 ${w} ${h}`} style={{ display: 'block', overflow: 'visible' }}>
      <path d={area} fill={color} fillOpacity="0.10"/>
      <path d={d} fill="none" stroke={color} strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
      <circle cx={last[0]} cy={last[1]} r="2.75" fill={color}/>
      <circle cx={last[0]} cy={last[1]} r="5" fill={color} fillOpacity="0.18"/>
    </svg>
  );
};

const formatDuration = (ms) => {
  const totalS = Math.max(0, Math.round(ms / 1000));
  if (totalS < 60) return [`${totalS}s`, ''];
  const m = Math.floor(totalS / 60);
  const s = totalS % 60;
  if (m < 60) return [`${m}m ${String(s).padStart(2, '0')}s`, ''];
  return [`${(m / 60).toFixed(1)}h`, ''];
};

const formatCount = (n) => {
  if (n < 10_000) return [n.toLocaleString('en-US'), ''];
  if (n < 1_000_000) return [(n / 1000).toFixed(1), 'k'];
  return [(n / 1_000_000).toFixed(2), 'M'];
};

const seedSeries = (n, base, jitter) =>
  Array.from({ length: n }, (_, i) => {
    const drift = Math.sin(i / 2.3) * jitter * 0.5;
    const noise = (Math.random() - 0.5) * jitter;
    return Math.max(0, base + drift + noise);
  });

const MetricCard = ({ label, value, unit, subline, sparkData, sparkColor, big, mascot }) => (
  <div style={{
    background: '#fff',
    border: '1px solid var(--border)',
    borderRadius: 14,
    padding: '18px 20px 18px',
    boxShadow: 'var(--shadow-sm)',
    display: 'flex', flexDirection: 'column', gap: 14,
    position: 'relative', overflow: 'hidden',
  }}>
    {mascot && (
      <img
        src={mascot.src}
        alt=""
        width={mascot.size || 44}
        height={mascot.size || 44}
        style={{
          position: 'absolute',
          top: mascot.top ?? 10,
          right: mascot.right ?? 12,
          opacity: mascot.opacity ?? 0.85,
          transform: mascot.rotate ? `rotate(${mascot.rotate}deg)` : 'none',
          pointerEvents: 'none',
        }}
      />
    )}
    <div style={{
      fontFamily: 'var(--font-sans)', fontSize: 11.5, fontWeight: 600,
      textTransform: 'uppercase', letterSpacing: '0.1em',
      color: 'var(--fg3)', lineHeight: 1.3,
      paddingRight: mascot ? 56 : 0,
    }}>{label}</div>
    <div style={{
      fontFamily: 'var(--font-serif)', fontWeight: 600,
      fontSize: 44, lineHeight: 1.05, letterSpacing: '-0.02em',
      color: 'var(--fg1)',
      display: 'flex', alignItems: 'baseline', gap: 6,
      ...(big ? {
        fontFamily: 'var(--font-mono)', fontSize: 18, fontWeight: 600,
        letterSpacing: 0, lineHeight: 1.4,
        whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
        display: 'block',
      } : null),
    }}>
      {value}
      {unit && <span style={{ fontSize: '0.5em', color: 'var(--fg3)', fontWeight: 500, fontFamily: 'var(--font-sans)' }}>{unit}</span>}
    </div>
    {subline && (
      <div style={{
        fontFamily: 'var(--font-sans)', fontSize: 12.5,
        color: 'var(--fg3)', lineHeight: 1.4, marginTop: -4,
      }}>{subline}</div>
    )}
    {sparkData && (
      <Sparkline data={sparkData} color={sparkColor} w={300} h={28}/>
    )}
  </div>
);

const TOP_PAGES = [
  '/docs/get-started/quickstart/',
  '/docs/rolling-counters/',
  '/learn/chapter-1/',
  '/docs/get-started/',
  '/learn/fraud-rules/',
];

const LiveMetrics = () => {
  const [dwellMs, setDwellMs] = React.useState(134_000);   // ~2m 14s
  const [pageViews, setPageViews] = React.useState(1_247);
  const [topPageIdx, setTopPageIdx] = React.useState(0);
  const [topPageHits, setTopPageHits] = React.useState(382);

  // 60 points · 1 per minute over the last hour
  const [dwellSeries, setDwellSeries] = React.useState(() => seedSeries(60, 130_000, 25_000));
  // 24 points · 1 per hour over the last 24 hours
  const [pvSeries, setPvSeries] = React.useState(() => seedSeries(24, 50, 18));

  React.useEffect(() => {
    const tick = () => {
      setDwellMs(v => Math.max(40_000, v + (Math.random() - 0.45) * 14_000));
      setPageViews(v => v + Math.floor(1 + Math.random() * 4));
      setTopPageHits(v => Math.max(80, v + Math.floor((Math.random() - 0.4) * 12)));
      if (Math.random() < 0.15) setTopPageIdx(i => (i + 1) % TOP_PAGES.length);
      setDwellSeries(s => [...s.slice(1), Math.max(40_000, s[s.length - 1] + (Math.random() - 0.5) * 18_000)]);
      setPvSeries(s => [...s.slice(1), Math.max(0, s[s.length - 1] + (Math.random() - 0.3) * 12)]);
    };
    const id = setInterval(tick, 5000);
    return () => clearInterval(id);
  }, []);

  const dwellTickedMs = useTickingNumber(dwellMs, { duration: 900 });
  const pvTicked = useTickingNumber(pageViews, { duration: 700 });
  const hitsTicked = useTickingNumber(topPageHits, { duration: 700 });

  const [dwellNum, dwellUnit] = formatDuration(dwellTickedMs);
  const [pvNum, pvUnit] = formatCount(pvTicked);

  return (
    <div style={{ position: 'relative' }}>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr', gap: 14 }}>
        <MetricCard
          label="Avg time on /docs/ · Last hour"
          value={dwellNum}
          unit={dwellUnit}
          sparkData={dwellSeries}
          sparkColor="var(--accent)"
          mascot={{ src: '../../assets/mascot-work-pose.svg', size: 48, top: 8, right: 10, rotate: -4 }}
        />
        <MetricCard
          label="Pages viewed · Last 24h"
          value={pvNum}
          unit={pvUnit}
          sparkData={pvSeries}
          sparkColor="var(--beava-info)"
          mascot={{ src: '../../assets/mascot-pose-2.svg', size: 48, top: 8, right: 10, rotate: 4 }}
        />
        <MetricCard
          big
          label="Top page · Last hour"
          mascot={{ src: '../../assets/mascot-pose-3.svg', size: 44, top: 10, right: 12, rotate: -2 }}
          value={
            <span key={topPageIdx} style={{
              fontFamily: 'var(--font-mono)', fontSize: 17, fontWeight: 600,
              color: 'var(--fg1)', letterSpacing: 0,
              animation: 'beava-fade-up 360ms var(--ease-out)',
              display: 'block',
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              maxWidth: '100%',
            }}>
              {TOP_PAGES[topPageIdx]}
            </span>
          }
          subline={<><span style={{ fontFamily: 'var(--font-mono)', color: 'var(--fg2)' }}>{hitsTicked}</span> views</>}
        />
      </div>

      <p style={{
        margin: '16px 4px 0',
        fontFamily: 'var(--font-sans)', fontSize: 13, color: 'var(--fg3)',
        fontStyle: 'italic', lineHeight: 1.5,
      }}>
        Three real beava queries. <a href="#pipeline" style={{
          color: 'var(--accent)', textDecoration: 'none',
          borderBottom: '1px solid color-mix(in oklab, var(--accent) 35%, transparent)',
          fontStyle: 'normal', fontWeight: 500,
        }}>The pipeline is below ↓</a>
      </p>
    </div>
  );
};

// ----- InstallTabs -----
const INSTALL_TABS = [
  { id: 'brew',   label: 'brew',   cmd: 'brew install beava' },
  { id: 'pip', label: 'pip', cmd: 'pip install beava' },
  { id: 'docker', label: 'docker', cmd: 'docker run -p 6400:6400 beava/beava:latest' },
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
      <div style={{
        fontFamily: 'var(--font-sans)', fontSize: 13.5, color: 'var(--fg2)',
        fontWeight: 500, marginBottom: 10, paddingLeft: 4,
      }}>
        Run it locally.
      </div>
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
      <div style={{
        marginTop: 12, paddingLeft: 4,
        fontFamily: 'var(--font-sans)', fontSize: 13, color: 'var(--fg3)',
        lineHeight: 1.5,
      }}>
        ~14 MB <span style={{ color: 'var(--border-strong)' }}>·</span> macOS, Linux, Windows <span style={{ color: 'var(--border-strong)' }}>·</span> runs on 1 GB RAM <span style={{ color: 'var(--border-strong)' }}>·</span> scales to one big box
      </div>
      <div style={{
        marginTop: 14, paddingLeft: 4,
        fontFamily: 'var(--font-sans)', fontSize: 14.5, color: 'var(--fg2)',
        lineHeight: 1.5, fontWeight: 500,
      }}>
        Push events <span style={{ color: 'var(--border-strong)', margin: '0 4px' }}>·</span>
        Maintain tables <span style={{ color: 'var(--border-strong)', margin: '0 4px' }}>·</span>
        Query by key.
      </div>
    </div>
  );
};

const Hero = () => {
  return (
    <section style={{ padding: '56px 24px 88px', position: 'relative' }}>
      <div style={{
        maxWidth: 1200, margin: '0 auto',
        display: 'grid', gridTemplateColumns: '1.15fr 1fr', gap: 72, alignItems: 'center',
      }} className="beava-hero-grid">
        <div>
          <div style={{
            display: 'inline-flex', alignItems: 'center', gap: 10,
            padding: '5px 12px 5px 10px', borderRadius: 999,
            background: 'var(--beava-orange-wash)', border: '1px solid #f1d8c2',
            color: 'var(--accent)', fontSize: 12.5, fontWeight: 600,
            marginBottom: 24, fontFamily: 'var(--font-sans)',
            whiteSpace: 'nowrap', flexWrap: 'nowrap',
          }}>
            <span style={{
              width: 6, height: 6, background: 'var(--accent)', borderRadius: 999,
              boxShadow: '0 0 0 3px rgba(184,92,32,0.18)',
            }}/>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12 }}>v0.9.4</span>
            <span style={{ color: '#d8b594' }}>·</span>
            <span>Apache 2.0</span>
            <span style={{ color: '#d8b594' }}>·</span>
            <span>single binary</span>
          </div>

          <div style={{ marginBottom: 12 }}>
            <span style={{
              fontFamily: 'var(--font-accent)', fontWeight: 700,
              fontSize: 24, lineHeight: 1, color: 'var(--accent)',
              transform: 'rotate(-2deg)', transformOrigin: 'left center',
              display: 'inline-block',
            }}>
              Dam good at streams.
            </span>
          </div>

          <h1 style={{
            fontFamily: 'var(--font-serif)', fontWeight: 600,
            fontSize: 'clamp(36px, 4.6vw, 60px)',
            lineHeight: 1.2, letterSpacing: '-0.02em',
            color: 'var(--fg1)', margin: '0 0 44px',
            maxWidth: 600, textWrap: 'balance',
          }}>
            Real-time features without heavy infrastructure.
          </h1>

          <p style={{
            fontFamily: 'var(--font-sans)', fontWeight: 400,
            fontSize: 17, lineHeight: 1.55,
            color: 'var(--fg2)', margin: '0 0 18px',
            maxWidth: 560, textWrap: 'pretty',
          }}>
            If your last &lsquo;simple&rsquo; feature ate a sprint to deploy, you know why we built this.
          </p>

          <p style={{
            fontFamily: 'var(--font-serif)', fontStyle: 'italic',
            fontWeight: 400, fontSize: 22, lineHeight: 1.45,
            color: 'var(--fg1)', margin: '0 0 36px',
            maxWidth: 560, textWrap: 'pretty',
          }}>
            Personalization, fraud rules, live dashboards &mdash; in hours, not quarters.
          </p>

          <InstallTabs/>

          <div style={{ display: 'flex', gap: 16, alignItems: 'center', marginTop: 8, flexWrap: 'wrap' }}>
            <Button variant="primary" size="lg" icon={<Icon name="arrow" size={16}/>}>
              Read the guide
            </Button>
          </div>
        </div>

        <div style={{ position: 'relative' }}>
          {/* Mascot peeking from the top of the live-metrics panel */}
          <img
            src="../../assets/mascot-pose-2.svg"
            alt=""
            width={110}
            height={110}
            style={{
              position: 'absolute',
              top: -78, right: -10,
              transform: 'rotate(8deg)',
              filter: 'drop-shadow(0 6px 14px rgba(26,23,20,0.12))',
              pointerEvents: 'none',
              zIndex: 2,
            }}
            className="beava-hero-mascot"
          />
          <LiveMetrics/>
        </div>
      </div>

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
          .beava-hero-mascot {
            display: none !important;
          }
        }
      `}</style>
    </section>
  );
};
window.Hero = Hero;
window.LiveMetrics = LiveMetrics;
