// ui_kits/docs/DocsContent.jsx
const H1 = ({ children }) => <h1 style={{ fontFamily: 'var(--font-serif)', fontWeight: 600, fontSize: 44, lineHeight: 1.1, letterSpacing: '-0.02em', color: 'var(--fg1)', margin: '0 0 12px', fontFeatureSettings: '"ss01" on' }}>{children}</h1>;
const H2 = ({ id, children }) => <h2 id={id} style={{ fontFamily: 'var(--font-serif)', fontWeight: 600, fontSize: 28, lineHeight: 1.25, letterSpacing: '-0.015em', color: 'var(--fg1)', margin: '40px 0 12px', fontFeatureSettings: '"ss01" on' }}>{children}</h2>;
const H3 = ({ id, children }) => <h3 id={id} style={{ fontFamily: 'var(--font-sans)', fontWeight: 600, fontSize: 19, color: 'var(--fg1)', margin: '28px 0 8px' }}>{children}</h3>;
const P = ({ children }) => <p style={{ fontFamily: 'var(--font-sans)', fontSize: 16, lineHeight: 1.75, color: 'var(--fg1)', margin: '0 0 16px', textWrap: 'pretty' }}>{children}</p>;
const Inline = ({ children }) => <code style={{ fontFamily: 'var(--font-mono)', fontSize: '0.92em', background: 'var(--bg-inset)', color: 'var(--code-fg)', padding: '0.12em 0.4em', borderRadius: 4, border: '1px solid var(--border-inset)' }}>{children}</code>;

const DocsContent = () => (
  <main style={{ flex: 1, maxWidth: 780, padding: '32px 48px', minWidth: 0 }}>
    <div style={{ fontFamily: 'var(--font-mono)', fontSize: 12.5, color: 'var(--fg3)', marginBottom: 18 }}>
      Features <span style={{ color: 'var(--border-strong)', margin: '0 8px' }}>/</span> Rolling counters
    </div>

    <H1>Rolling counters</H1>
    <p style={{ fontFamily: 'var(--font-sans)', fontSize: 19, lineHeight: 1.55, color: 'var(--fg2)', margin: '0 0 22px', textWrap: 'pretty' }}>
      A rolling counter answers the question: <em>"how many of X have happened in the last N seconds?"</em> — a useful primitive for rate limits, trending items, and surge detection.
    </p>

    <DocsPageMeta stable="v0.9" updated="3 days ago" readMins={6}/>
    <DocsRelatedChips items={[
      { label: 'velocities' },
      { label: 'rate limits' },
      { label: 'last-N-seen' },
      { label: 'leaderboards' },
    ]}/>

    <H2 id="what">What is a rolling counter?</H2>
    <P>
      In beava, a rolling counter is a named feature defined in your config. It maintains a sliding time window over an event stream, keyed on a field you pick. Queries return the count within the window, computed at query time.
    </P>
    <P>
      Counters are cheap: memory is <Inline>O(keys × windowFreshness)</Inline>, and queries are sub-millisecond under typical load. You can define hundreds of them without thinking about it.
    </P>

    <H3 id="define">Defining a counter</H3>
    <P>Add a feature to your <Inline>beava.toml</Inline>:</P>
    <CodeBlock lang="toml">
<C># ~/beava.toml{'\n'}</C>
<K>[features.clicks_60s]</K>{'\n'}
<T>type</T>   = <S>"rolling_counter"</S>{'\n'}
<T>window</T> = <S>"60s"</S>{'\n'}
<T>key</T>    = <S>"user_id"</S>{'\n'}
<T>source</T> = <S>"events.click"</S>
    </CodeBlock>

    <Callout kind="note" title="Note">
      Feature keys are case-sensitive and cannot contain slashes. They must match <Inline>^[a-z][a-z0-9_]*$</Inline>.
    </Callout>

    <H3 id="push">Pushing events</H3>
    <P>Push events from anywhere you can make an HTTP request. Clients exist for Node, Go, Python, and Ruby; the HTTP endpoint is stable and documented.</P>
    <CodeBlock lang="javascript">
<C>// in your CRUD app{'\n'}</C>
<K>await</K> beava.<F>push</F>(<S>"click"</S>, &#123; user_id: <N>42</N>, item: <S>"button-a"</S> &#125;);
    </CodeBlock>

    <H3 id="query">Querying</H3>
    <P>Read the counter at any time:</P>
    <CodeBlock lang="javascript">
<K>const</K> n = <K>await</K> beava.<F>get</F>(<S>"clicks_60s"</S>, &#123; user_id: <N>42</N> &#125;);{'\n'}
<C>// =&gt; 7{'\n'}</C>
{'\n'}
<C>// or in bulk{'\n'}</C>
<K>const</K> rows = <K>await</K> beava.<F>getMany</F>(<S>"clicks_60s"</S>, [<N>42</N>, <N>43</N>, <N>44</N>]);
    </CodeBlock>

    <Callout kind="tip" title="Tip">
      Pre-warm hot keys with <Inline>beava warm</Inline> before a known traffic spike. It's a cheap way to avoid a cold-cache tail on launch.
    </Callout>

    <H2 id="perf">Performance</H2>
    <P>
      Rolling counters are backed by a ring-buffer per key. At query time, we sum over the active slots — typically 4–16 depending on window size. No background aggregation, no compaction stalls.
    </P>
    <P>
      On a single 4-core instance, we observe ~180k queries/sec at p99 under 2ms with 1M active keys. See <a style={{ color: 'var(--accent)' }}>benchmarks</a> for the full matrix.
    </P>

    <H2 id="caveats">Caveats</H2>
    <Callout kind="warn" title="Heads up">
      Windows shorter than 1s are experimental and may drift under heavy load. We recommend ≥5s in production.
    </Callout>
    <P>
      Counters are <strong>not</strong> durable across restarts by default — they recover by replay, which takes ~30s on typical volumes. See <a style={{ color: 'var(--accent)' }}>durability</a> if you need stricter guarantees.
    </P>

    <DocsHelpCallout/>

    <div style={{ marginTop: 48, paddingTop: 24, borderTop: '1px solid var(--border)', display: 'flex', justifyContent: 'space-between' }}>
      <a style={{ display: 'flex', flexDirection: 'column', gap: 4, padding: 16, border: '1px solid var(--border)', borderRadius: 10, textDecoration: 'none', minWidth: 200 }}>
        <span style={{ fontFamily: 'var(--font-sans)', fontSize: 12, color: 'var(--fg3)' }}>← Previous</span>
        <span style={{ fontFamily: 'var(--font-sans)', fontSize: 15, fontWeight: 600, color: 'var(--fg1)' }}>Configuration</span>
      </a>
      <a style={{ display: 'flex', flexDirection: 'column', gap: 4, padding: 16, border: '1px solid var(--border)', borderRadius: 10, textDecoration: 'none', minWidth: 200, textAlign: 'right' }}>
        <span style={{ fontFamily: 'var(--font-sans)', fontSize: 12, color: 'var(--fg3)' }}>Next →</span>
        <span style={{ fontFamily: 'var(--font-sans)', fontSize: 15, fontWeight: 600, color: 'var(--accent)' }}>Velocities</span>
      </a>
    </div>
  </main>
);
window.DocsContent = DocsContent;
