// js/Shared.jsx — icons, buttons, eyebrows, nav, footer

const Icon = ({ name, size = 20, stroke = 1.75, ...rest }) => {
  const paths = {
    arrow:   <><path d="M5 12h14"/><path d="M13 6l6 6-6 6"/></>,
    star:    <path d="M12 2l3.1 6.3 6.9 1-5 4.9 1.2 6.8L12 17.8 5.8 21l1.2-6.8-5-4.9 6.9-1z"/>,
    check:   <path d="M20 6L9 17l-5-5"/>,
    copy:    <><rect x="9" y="9" width="12" height="12" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></>,
    github:  <path d="M12 2a10 10 0 0 0-3.16 19.49c.5.09.68-.22.68-.48v-1.7c-2.78.6-3.37-1.34-3.37-1.34-.46-1.16-1.11-1.47-1.11-1.47-.91-.62.07-.6.07-.6 1 .07 1.53 1.03 1.53 1.03.89 1.53 2.34 1.09 2.91.83.09-.65.35-1.09.63-1.34-2.22-.25-4.56-1.11-4.56-4.94 0-1.09.39-1.98 1.03-2.68-.1-.26-.45-1.27.1-2.65 0 0 .84-.27 2.75 1.02a9.56 9.56 0 0 1 5 0c1.91-1.29 2.75-1.02 2.75-1.02.55 1.38.2 2.39.1 2.65.64.7 1.03 1.59 1.03 2.68 0 3.84-2.34 4.68-4.57 4.93.36.31.68.92.68 1.85v2.75c0 .27.18.58.69.48A10 10 0 0 0 12 2z"/>,
    zap:     <path d="M13 2L3 14h9l-1 8 10-12h-9z"/>,
    terminal:<><path d="M4 17l6-6-6-6"/><path d="M12 19h8"/></>,
    book:    <><path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20"/><path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z"/></>,
  };
  const filled = name === 'star' || name === 'zap' || name === 'github';
  return (
    <svg width={size} height={size} viewBox="0 0 24 24"
         fill={filled ? 'currentColor' : 'none'}
         stroke="currentColor" strokeWidth={stroke}
         strokeLinecap="round" strokeLinejoin="round" {...rest}>
      {paths[name]}
    </svg>
  );
};

const Button = ({ variant = 'primary', size = 'md', children, icon, iconLeft, href, ...rest }) => {
  const base = {
    fontFamily: 'var(--font-sans)', fontWeight: 600,
    borderRadius: 10, border: '1px solid transparent',
    cursor: 'pointer', transition: 'all 200ms cubic-bezier(0.22,1,0.36,1)',
    display: 'inline-flex', alignItems: 'center', gap: 8, lineHeight: 1,
    textDecoration: 'none', whiteSpace: 'nowrap',
  };
  const sizes = {
    sm: { padding: '7px 12px', fontSize: 13, borderRadius: 8 },
    md: { padding: '10px 18px', fontSize: 14 },
    lg: { padding: '15px 24px', fontSize: 16 },
  };
  const variants = {
    primary:   { background: 'var(--accent)', color: '#fff', boxShadow: '0 2px 0 rgba(26,23,20,0.12)' },
    secondary: { background: '#fff', color: 'var(--fg1)', borderColor: 'var(--border)' },
    ghost:     { background: 'transparent', color: 'var(--fg1)' },
  };
  const El = href ? 'a' : 'button';
  return <El href={href} style={{ ...base, ...sizes[size], ...variants[variant] }} {...rest}>{iconLeft}{children}{icon}</El>;
};

const Eyebrow = ({ children, style }) => (
  <div style={{ fontFamily: 'var(--font-sans)', fontWeight: 600, fontSize: 12,
                textTransform: 'uppercase', letterSpacing: '0.08em',
                color: 'var(--accent)', ...style }}>{children}</div>
);

// Callout — reusable card used through the field guide to punctuate prose
// with a claim, insight, warning, or action prompt. Cream card with orange
// eyebrow, serif body, small mascot top-right.
// Variants: eyebrow label + mascot pose; tint="warm" for a soft-orange bg.
const Callout = ({ eyebrow = 'NOTE', mascot = 'work-pose', tint = 'paper', children, style = {} }) => {
  // Per-pose config — src + native aspect-preserving dimensions. The
  // geometric mark is 1536x1024 (3:2 landscape), so it gets a wider box
  // and is scaled up a bit since user asked for more presence.
  const mascotConf = {
    'work-pose': { src: '/assets/mascot-work-pose.svg',     w: 104, h: 104 },
    'geometric': { src: '/assets/mascot-mark-geometric.png', w: 168, h: 112 },
    'pose-2':    { src: '/assets/mascot-pose-2.svg',         w: 104, h: 104 },
    'pose-3':    { src: '/assets/mascot-pose-3.svg',         w: 104, h: 104 },
  };
  const conf = mascotConf[mascot] || mascotConf['work-pose'];
  const bg = tint === 'warm' ? 'var(--beava-orange-wash)' : 'var(--beava-paper)';
  return (
    <div style={{
      background: bg,
      border: '1px solid var(--border)',
      borderRadius: 18,
      padding: '24px 30px',
      margin: '36px 0',
      position: 'relative',
      boxShadow: 'var(--shadow-xs)',
      ...style,
    }}>
      <img src={conf.src} alt="" width={conf.w} height={conf.h} style={{
        position: 'absolute',
        top: 0,
        right: 0,
        transform: 'translate(50%, -50%) rotate(6deg)',
        opacity: 1,
        pointerEvents: 'none',
      }}/>
      <div>
        <div style={{
          fontFamily: 'var(--font-sans)',
          fontSize: 12, fontWeight: 700,
          textTransform: 'uppercase',
          letterSpacing: '0.1em',
          color: 'var(--accent)',
          marginBottom: 10,
        }}>{eyebrow}</div>
        <div style={{
          fontFamily: 'var(--font-serif)',
          fontSize: 19, lineHeight: 1.45,
          color: 'var(--fg1)',
          textWrap: 'pretty',
        }}>{children}</div>
      </div>
    </div>
  );
};

// Banner — dismissable top bar for announcements (cloud waitlist, launch, etc).
// Dismiss state lives in localStorage with a 30-day TTL, then re-shows. Pass
// an `id` to version dismissals — bump the id when copy changes to re-trigger.
const Banner = ({ id = 'cloud-waitlist-v1', emoji = '☁️', children, href = '/cloud' }) => {
  const [dismissed, setDismissed] = React.useState(true); // default hidden to avoid flash
  React.useEffect(() => {
    try {
      const raw = localStorage.getItem('banner:' + id);
      if (!raw) { setDismissed(false); return; }
      const parsed = JSON.parse(raw);
      const THIRTY_DAYS = 30 * 24 * 60 * 60 * 1000;
      if (Date.now() - parsed.ts > THIRTY_DAYS) setDismissed(false);
    } catch (_) { setDismissed(false); }
  }, [id]);

  const dismiss = (e) => {
    e.preventDefault(); e.stopPropagation();
    try { localStorage.setItem('banner:' + id, JSON.stringify({ ts: Date.now() })); } catch (_) {}
    setDismissed(true);
  };

  if (dismissed) return null;

  return (
    <a href={href} style={{
      display: 'block', textDecoration: 'none', color: 'var(--fg1)',
      background: 'var(--beava-paper)', borderBottom: '1px solid var(--border)',
      padding: '10px 52px 10px 28px',
      fontFamily: 'var(--font-sans)', fontSize: 13.5,
      textAlign: 'center', position: 'relative', zIndex: 60,
    }}>
      <span style={{ marginRight: 8 }}>{emoji}</span>
      <span style={{ color: 'var(--fg1)' }}>{children}</span>
      <span style={{ color: 'var(--accent)', fontWeight: 600, marginLeft: 6 }}>→</span>
      <button onClick={dismiss} aria-label="Dismiss" style={{
        position: 'absolute', top: '50%', right: 16,
        transform: 'translateY(-50%)',
        background: 'transparent', border: 'none', cursor: 'pointer',
        color: 'var(--fg3)', fontSize: 20, lineHeight: 1,
        padding: '2px 8px', borderRadius: 6,
      }}>×</button>
    </a>
  );
};

// Nav — shared across pages. active=current route id.
const Nav = ({ active = 'home' }) => {
  const [scrolled, setScrolled] = React.useState(false);
  React.useEffect(() => {
    const on = () => setScrolled(window.scrollY > 20);
    window.addEventListener('scroll', on); on();
    return () => window.removeEventListener('scroll', on);
  }, []);

  const navStyle = {
    position: 'sticky', top: 0, zIndex: 50, height: 64,
    background: scrolled ? 'rgba(253,250,244,0.90)' : 'transparent',
    backdropFilter: scrolled ? 'blur(10px)' : 'none',
    WebkitBackdropFilter: scrolled ? 'blur(10px)' : 'none',
    borderBottom: '1px solid ' + (scrolled ? 'var(--border)' : 'transparent'),
    display: 'flex', alignItems: 'center', padding: '0 28px',
    transition: 'all 200ms',
  };
  const link = (id) => ({
    padding: '6px 10px', borderRadius: 8, fontSize: 14,
    color: active === id ? 'var(--accent)' : 'var(--fg2)',
    textDecoration: 'none', fontWeight: 500,
    fontFamily: 'var(--font-sans)',
  });

  return (
    <nav style={navStyle}>
      <div style={{ maxWidth: 1200, margin: '0 auto', display: 'flex', alignItems: 'center', gap: 24, width: '100%' }}>
        <a href="/" style={{ display: 'flex', alignItems: 'center', gap: 10, color: 'var(--fg1)', textDecoration: 'none' }}>
          <img src="/assets/mascot-mark-geometric-transparent.png" alt="" width={32} height={32} style={{ display: 'block' }}/>
          <span style={{ fontFamily: 'var(--font-serif)', fontWeight: 600, fontStyle: 'italic', fontSize: 26, letterSpacing: '-0.025em', lineHeight: 1 }}>beava</span>
        </a>
        <div style={{ display: 'flex', gap: 4, marginLeft: 14, flex: 1 }}>
          <a style={link('guide')} href="/guide/">Guide</a>
          <a style={link('docs')} href="/docs/">Docs</a>
          <a style={link('community')} href="/community/">Community</a>
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <a href="https://github.com/beava-dev/beava" target="_blank" rel="noopener" style={{ display: 'flex', alignItems: 'center', gap: 6, fontSize: 13, color: 'var(--fg2)', padding: '7px 11px', border: '1px solid var(--border)', borderRadius: 8, background: '#fff', fontWeight: 500, textDecoration: 'none', fontFamily: 'var(--font-sans)' }}>
            <Icon name="github" size={14}/>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12 }}>8.2k ★</span>
          </a>
        </div>
      </div>
    </nav>
  );
};

// Footer — shared
const Footer = () => {
  const col = { fontFamily: 'var(--font-sans)', fontSize: 12, fontWeight: 700, textTransform: 'uppercase', letterSpacing: '0.08em', color: 'var(--fg3)', marginBottom: 14 };
  const lnk = { fontFamily: 'var(--font-sans)', fontSize: 14, color: 'var(--fg2)', textDecoration: 'none', display: 'block', padding: '3px 0' };
  return (
    <footer style={{ background: 'var(--bg-alt)', borderTop: '1px solid var(--border)', padding: '72px 24px 40px', marginTop: 0 }}>
      <div style={{ maxWidth: 1200, margin: '0 auto' }}>
        <div style={{ display: 'grid', gridTemplateColumns: '1.5fr 1fr 1fr 1fr', gap: 48, marginBottom: 56 }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 16 }}>
              <img src="/assets/logo-mark.png" width={44} height={44}/>
              <span style={{ fontFamily: 'var(--font-sans)', fontWeight: 700, fontSize: 22, color: 'var(--fg1)' }}>beava</span>
            </div>
            <p style={{ fontFamily: 'var(--font-sans)', fontSize: 15, lineHeight: 1.6, color: 'var(--fg2)', margin: '0 0 16px', maxWidth: 320 }}>
              Apache 2.0. One binary. Made with too much coffee.
            </p>
            <div style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--fg3)', lineHeight: 1.8 }}>
              <div>(Refresh the page for a fresh beaver</div>
              <div>and a new session id.)</div>
            </div>
          </div>
          <div>
            <div style={col}>Product</div>
            <a style={lnk} href="#">Docs</a>
            <a style={lnk} href="#">Changelog</a>
            <a style={lnk} href="#">Roadmap</a>
            <a style={lnk} href="#">Benchmarks</a>
          </div>
          <div>
            <div style={col}>Learn</div>
            <a style={lnk} href="field-guide-ch1.html">Field guide</a>
            <a style={lnk} href="#">Blog</a>
            <a style={lnk} href="#">Examples</a>
            <a style={lnk} href="#">Reference</a>
          </div>
          <div>
            <div style={col}>Community</div>
            <a style={lnk} href="#">GitHub</a>
            <a style={lnk} href="#">Discord</a>
            <a style={lnk} href="#">Talks</a>
            <a style={lnk} href="#">Brand kit</a>
          </div>
        </div>
        <div style={{ borderTop: '1px solid var(--border)', paddingTop: 22, display: 'flex', justifyContent: 'space-between', alignItems: 'center', flexWrap: 'wrap', gap: 12 }}>
          <div style={{ fontFamily: 'var(--font-sans)', fontSize: 13, color: 'var(--fg3)' }}>
            © 2026 beava labs · Apache 2.0 · not VC-funded, not your lord
          </div>
          <div style={{ display: 'flex', gap: 14, color: 'var(--fg3)' }}>
            <a href="#" style={{ color: 'inherit' }}><Icon name="github" size={18}/></a>
            <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12, color: 'var(--fg3)' }}>beava.dev</span>
          </div>
        </div>
      </div>
    </footer>
  );
};

// Copy-to-clipboard button for code blocks
const CopyBtn = ({ text }) => {
  const [copied, setCopied] = React.useState(false);
  const onClick = () => {
    navigator.clipboard?.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };
  return (
    <button onClick={onClick} aria-label="Copy" style={{
      position: 'absolute', top: 10, right: 10,
      background: '#fff', border: '1px solid var(--border)',
      color: copied ? 'var(--beava-success)' : 'var(--fg3)',
      borderRadius: 8, padding: '6px 10px', fontSize: 12,
      fontFamily: 'var(--font-sans)', fontWeight: 600,
      cursor: 'pointer', display: 'inline-flex', alignItems: 'center', gap: 6,
      transition: 'all 160ms',
    }}>
      <Icon name={copied ? 'check' : 'copy'} size={13} stroke={2}/>
      {copied ? 'copied' : 'copy'}
    </button>
  );
};

// DfPanel — renders a beava stream/table as a pandas-style dataframe.
// Fully bordered cells + row index column + accent-orange header row.
// Warm palette. Home page defines a local copy that shadows this one;
// anywhere else (Chapter 1, recipes), import from here.
const DfPanel = ({ title, columns, rows, ellipsis = false, minHeight }) => {
  const cellBorder = '1px solid var(--border-strong)';
  const headerCell = {
    padding: '8px 10px', textAlign: 'center',
    fontFamily: 'var(--font-mono)', fontWeight: 700, fontSize: 11,
    color: 'var(--beava-cream)', letterSpacing: '0.02em',
    background: 'var(--accent)', border: '1px solid var(--accent)',
    whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
  };
  const indexCell = {
    padding: '7px 10px', textAlign: 'center',
    fontFamily: 'var(--font-mono)', fontWeight: 600, fontSize: 12,
    color: 'var(--fg3)', background: 'var(--beava-cream-deep)',
    border: cellBorder, width: 36,
  };
  const dataCell = (i) => ({
    padding: '7px 10px', textAlign: i === 0 ? 'left' : 'right',
    fontFamily: 'var(--font-mono)', fontSize: 12.5,
    color: i === 0 ? 'var(--accent)' : 'var(--fg1)',
    fontWeight: i === 0 ? 600 : 400,
    background: 'var(--bg-card)', border: cellBorder,
    whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis',
  });
  return (
    <div style={{ display: 'flex', flexDirection: 'column', minHeight }}>
      <div style={{ padding: '0 2px 8px', fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--fg3)', letterSpacing: '0.03em' }}>{title}</div>
      <table style={{ width: '100%', borderCollapse: 'collapse', tableLayout: 'fixed', border: '2px solid var(--accent)', boxShadow: 'var(--shadow-xs)' }}>
        <thead>
          <tr>
            <th style={{ ...headerCell, width: 36 }}></th>
            {columns.map((c) => (<th key={c} style={headerCell}>{c}</th>))}
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 && (
            <tr>
              <td style={{ ...indexCell, color: 'var(--fg3)', opacity: 0.45 }}>—</td>
              {columns.map((_, i) => (
                <td key={i} style={{ ...dataCell(i), color: 'var(--fg3)', opacity: 0.45, fontStyle: 'italic', fontWeight: 400 }}>
                  {i === 0 ? 'empty — send an event' : ''}
                </td>
              ))}
            </tr>
          )}
          {rows.map((r, rowIdx) => (
            <tr key={r.key} className="anim-row">
              <td style={indexCell}>{rowIdx}</td>
              {r.cells.map((c, i) => (<td key={i} style={dataCell(i)}>{c}</td>))}
            </tr>
          ))}
          {ellipsis && (
            <tr>
              <td style={{ ...indexCell, color: 'var(--fg3)', opacity: 0.55 }}>...</td>
              {columns.map((_, i) => (
                <td key={i} style={{ ...dataCell(i), color: 'var(--fg3)', opacity: 0.55, fontWeight: 400 }}>...</td>
              ))}
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
};

Object.assign(window, { Icon, Button, Eyebrow, Callout, Banner, Nav, Footer, CopyBtn, DfPanel });
