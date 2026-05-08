// js/sdk/SdkPager.jsx
//
// Bottom prev/next pager tiles for SDK pages.
//
// Mount:
//   <div id="bv-sdk-pager"></div>
//   ...
//   <script type="text/babel" src="/js/sdk/SdkPager.jsx"></script>
//   <script type="text/babel">
//     ReactDOM.createRoot(document.getElementById('bv-sdk-pager'))
//       .render(<SdkPager
//         prev={{ href: '/sdk/python/', label: 'Quickstart' }}
//         next={{ href: '/sdk/python/event/', label: '@bv.event (planned)' }}
//       />);
//   </script>
//
// Either side can be `null` (omitted on first/last pages). When
// only one side is set, the other slot stays empty — the grid
// preserves layout so the present tile stays in its column.

const SdkPager = ({ prev = null, next = null }) => (
  <div className="pager">
    {prev ? (
      <a href={prev.href} className="prev">
        <div className="dir">← Prev</div>
        <div className="ttl">{prev.label}</div>
      </a>
    ) : <span/>}
    {next ? (
      <a href={next.href} className="next">
        <div className="dir">Next →</div>
        <div className="ttl">{next.label}</div>
      </a>
    ) : <span/>}
  </div>
);

window.SdkPager = SdkPager;
