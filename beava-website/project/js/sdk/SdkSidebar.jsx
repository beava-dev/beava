// js/sdk/SdkSidebar.jsx
//
// Shared sidebar for /sdk/* pages. Single source of truth for the
// SDK nav structure — each new SDK reference page picks `active`
// and the sidebar handles active-link state + planned-link styling.
//
// Mount:
//   <div id="bv-sdk-sidebar" class="sidebar"></div>
//   ...
//   <script type="text/babel" src="/js/sdk/SdkSidebar.jsx"></script>
//   <script type="text/babel">
//     ReactDOM.createRoot(document.getElementById('bv-sdk-sidebar'))
//       .render(<SdkSidebar active="app"/>);
//   </script>
//
// Props:
//   active — id of the active page; one of:
//     "quickstart" | "app" | "event" | "table" | "col-lit" |
//     "operators" | "errors" | "http-push" | "http-get" |
//     "http-register" | "http-wire-spec"
//
// A page is rendered as `planned` (italic, no real link) iff its
// entry below has href === "#". As pages land, flip "#" to the real
// URL and the planned styling drops automatically.

const SDK_NAV = [
  { heading: 'Start here', items: [
    { id: 'quickstart', label: 'Quickstart', href: '/sdk/python/' },
  ]},
  { heading: 'Server', items: [
    { id: 'server',     label: 'Configuration',      href: '/sdk/server/' },
  ]},
  { heading: 'Python SDK', items: [
    { id: 'app',        label: 'App client',         href: '/sdk/python/app/' },
    { id: 'event',      label: '@bv.event',          href: '/sdk/python/event/' },
    { id: 'table',      label: '@bv.table',          href: '/sdk/python/table/' },
    { id: 'col-lit',    label: 'bv.col / bv.lit',    href: '/sdk/python/col-lit/' },
    { id: 'operators',  label: 'Operator catalogue', href: '/sdk/python/operators/' },
    { id: 'errors',     label: 'Errors',             href: '/sdk/python/errors/' },
  ]},
  { heading: 'HTTP API', items: [
    { id: 'http-push',      label: 'POST /push',     href: '/sdk/http/push/' },
    { id: 'http-get',       label: 'POST /get',      href: '/sdk/http/get/' },
    { id: 'http-register',  label: 'POST /register', href: '/sdk/http/register/' },
    { id: 'http-wire-spec', label: 'Wire spec',      href: '/sdk/http/wire-spec/' },
  ]},
];

const SdkSidebar = ({ active = null }) => (
  <React.Fragment>
    <div className="product-switcher">
      <div className="swatch">b</div>
      <div className="label">SDK reference <span className="sub">· v0</span></div>
    </div>
    {SDK_NAV.map(section => (
      <React.Fragment key={section.heading}>
        <h4>{section.heading}</h4>
        <ul>
          {section.items.map(item => {
            const isPlanned = item.href === '#';
            const isActive  = item.id === active;
            const cls = [
              isActive  ? 'active'  : '',
              isPlanned ? 'planned' : '',
            ].filter(Boolean).join(' ');
            return (
              <li key={item.id} className={cls || undefined}>
                <a href={item.href}>{item.label}</a>
              </li>
            );
          })}
        </ul>
      </React.Fragment>
    ))}
  </React.Fragment>
);

window.SdkSidebar = SdkSidebar;
