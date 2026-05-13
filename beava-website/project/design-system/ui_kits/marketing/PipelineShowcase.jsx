// ui_kits/marketing/PipelineShowcase.jsx
// Swapped from FeedClick to SiteMetrics — produces the three numbers in the
// hero LiveMetrics panel above.

const PipelineShowcase = () => {
  const S = {
    kw:  { color: 'var(--code-keyword)' },
    str: { color: 'var(--code-string)' },
    cmt: { color: 'var(--code-comment)', fontStyle: 'italic' },
    fn:  { color: 'var(--code-fn)' },
    num: { color: 'var(--code-number)' },
    ty:  { color: 'var(--code-type)' },
  };
  return (
    <section id="pipeline" style={{ padding: '88px 24px 96px', background: 'var(--beava-cream-deep)', borderTop: '1px solid var(--border)', borderBottom: '1px solid var(--border)' }}>
      <div style={{ maxWidth: 1040, margin: '0 auto' }}>
        <div style={{ marginBottom: 36 }}>
          <Eyebrow>The homepage runs beava</Eyebrow>
          <h2 style={{
            fontFamily: 'var(--font-serif)', fontWeight: 600,
            fontSize: 44, lineHeight: 1.1, letterSpacing: '-0.02em',
            color: 'var(--fg1)', margin: '10px 0 14px', maxWidth: 720, textWrap: 'balance',
          }}>
            <span style={{ fontFamily: 'var(--font-mono)', fontWeight: 600 }}>13 lines.</span> That&rsquo;s the whole pipeline.
          </h2>
          <p style={{
            fontFamily: 'var(--font-sans)', fontSize: 17, lineHeight: 1.55,
            color: 'var(--fg2)', margin: 0, maxWidth: 640, textWrap: 'pretty',
          }}>
            Every page view on this site pushes a real event to this pipeline. Every number in the hero panel is a real feature query against it.
          </p>
        </div>

        <div style={{
          background: '#fff', border: '1px solid var(--border)', borderRadius: 16,
          boxShadow: 'var(--shadow-sm)', overflow: 'hidden',
        }}>
          <div style={{
            display: 'flex', alignItems: 'center', justifyContent: 'space-between',
            padding: '10px 16px', borderBottom: '1px solid var(--border)',
            background: 'var(--beava-paper)',
          }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
              <span style={{ width: 9, height: 9, borderRadius: 999, background: '#e6dccb' }}/>
              <span style={{ width: 9, height: 9, borderRadius: 999, background: '#e6dccb' }}/>
              <span style={{ width: 9, height: 9, borderRadius: 999, background: '#e6dccb' }}/>
              <span style={{ fontFamily: 'var(--font-mono)', fontSize: 12.5, color: 'var(--fg2)', marginLeft: 6 }}>
                site_metrics.py
              </span>
              <span style={{
                fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--fg3)',
                padding: '2px 8px', borderRadius: 999,
                background: '#fff', border: '1px solid var(--border)', marginLeft: 4,
                whiteSpace: 'nowrap',
              }}>13 lines</span>
            </div>
            <span style={{
              display: 'inline-flex', alignItems: 'center', gap: 6,
              fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--beava-success)',
              whiteSpace: 'nowrap',
            }}>
              <span style={{ width: 6, height: 6, borderRadius: 999, background: 'var(--beava-success)' }}/>
              registered on beava.dev
            </span>
          </div>

          <pre style={{
            margin: 0, padding: '22px 28px',
            fontFamily: 'var(--font-mono)', fontSize: 14, lineHeight: 1.7,
            color: 'var(--code-fg)', background: 'var(--code-bg)',
            border: 0, borderRadius: 0, boxShadow: 'none', overflowX: 'auto',
          }}>
<span style={S.kw}>import</span> beava <span style={S.kw}>as</span> bv{'\n'}
{'\n'}
<span style={S.fn}>@bv.stream</span>{'\n'}
<span style={S.kw}>class</span> <span style={S.ty}>PageView</span>:{'\n'}
{'    '}session_id: <span style={S.ty}>str</span>{'\n'}
{'    '}path: <span style={S.ty}>str</span>{'\n'}
{'    '}dwell_ms: <span style={S.ty}>int</span>   <span style={S.cmt}># set when the visitor leaves the page</span>{'\n'}
{'\n'}
<span style={S.fn}>@bv.table</span>(key=<span style={S.str}>"__global__"</span>){'\n'}
<span style={S.kw}>def</span> <span style={S.fn}>SiteMetrics</span>(e: <span style={S.ty}>PageView</span>):{'\n'}
{'    '}<span style={S.kw}>return</span> e.<span style={S.fn}>agg</span>({'\n'}
{'        '}<span style={{ background: 'rgba(184,92,32,0.10)', borderRadius: 3, padding: '0 2px' }}>avg_dwell_docs_1h</span> = bv.<span style={S.fn}>avg</span>(e.dwell_ms, window=<span style={S.str}>"1h"</span>,{'\n'}
{'                                   '}where=<span style={S.str}>"_event.path.startswith('/docs/')"</span>),{'\n'}
{'        '}<span style={{ background: 'rgba(58,106,138,0.10)', borderRadius: 3, padding: '0 2px' }}>page_views_today</span>  = bv.<span style={S.fn}>count</span>(window=<span style={S.str}>"24h"</span>),{'\n'}
{'        '}<span style={{ background: 'rgba(217,122,62,0.12)', borderRadius: 3, padding: '0 2px' }}>top_page_1h</span>       = bv.<span style={S.fn}>top_k</span>(e.path, k=<span style={S.num}>1</span>, window=<span style={S.str}>"1h"</span>),{'\n'}
{'    '}){'\n'}
{'\n'}
bv.<span style={S.fn}>App</span>(<span style={S.str}>"0.0.0.0:6400"</span>).<span style={S.fn}>register</span>(<span style={S.ty}>PageView</span>, <span style={S.fn}>SiteMetrics</span>).<span style={S.fn}>serve</span>()
          </pre>

          <div style={{
            display: 'grid', gridTemplateColumns: 'repeat(3, 1fr)', gap: 0,
            borderTop: '1px solid var(--border)', background: '#fff',
          }}>
            {[
              { color: 'rgba(184,92,32,0.10)', dot: 'var(--accent)',             label: 'avg_dwell_docs_1h', maps: 'Avg time on /docs/ · 1h' },
              { color: 'rgba(58,106,138,0.10)', dot: 'var(--beava-info)',        label: 'page_views_today',  maps: 'Pages viewed today' },
              { color: 'rgba(217,122,62,0.12)', dot: 'var(--beava-orange-soft)', label: 'top_page_1h',       maps: 'Top page · this hour' },
            ].map((row, i) => (
              <div key={row.label} style={{
                padding: '14px 18px',
                borderRight: i < 2 ? '1px solid var(--border)' : 'none',
                display: 'flex', flexDirection: 'column', gap: 4,
              }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                  <span style={{ width: 8, height: 8, borderRadius: 2, background: row.dot }}/>
                  <code style={{
                    fontFamily: 'var(--font-mono)', fontSize: 12.5,
                    background: row.color, padding: '2px 6px', borderRadius: 4,
                    color: 'var(--code-fg)', border: 0,
                  }}>{row.label}</code>
                </div>
                <div style={{ fontFamily: 'var(--font-sans)', fontSize: 12.5, color: 'var(--fg3)', paddingLeft: 16 }}>
                  → <span style={{ color: 'var(--fg2)' }}>{row.maps}</span> in the hero
                </div>
              </div>
            ))}
          </div>
        </div>

        <p style={{
          fontFamily: 'var(--font-sans)', fontSize: 14, color: 'var(--fg3)',
          margin: '20px 4px 0', textAlign: 'center', fontStyle: 'italic',
        }}>
          No tracking cookies, no fingerprinting. Anonymous session id per visit.
        </p>

        {/* Signed-by-the-beaver moment — small, warm, not in the way */}
        <div style={{
          marginTop: 28,
          display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 12,
        }}>
          <img src="../../assets/mascot-pose-3.svg" alt="" width={56} height={56} style={{ display: 'block' }}/>
          <div style={{
            fontFamily: 'var(--font-accent)', fontSize: 22, fontWeight: 700,
            color: 'var(--accent)', transform: 'rotate(-2deg)',
            transformOrigin: 'left center', display: 'inline-block',
            lineHeight: 1.1,
          }}>built it once, ships every page →</div>
        </div>
      </div>
    </section>
  );
};
window.PipelineShowcase = PipelineShowcase;
