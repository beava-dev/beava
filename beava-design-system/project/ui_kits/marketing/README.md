# Marketing UI Kit — beava.dev

Landing-page kit for beava.dev. Uses `colors_and_type.css` tokens.

## Components
- `Nav.jsx` — sticky top bar with brand, links, star count, CTA
- `Hero.jsx` — headline, subhead, dual-CTA, live decision feed, hand-drawn
  "live, right now" marker pointing at the feed, dollar-gain markers per row
- `Pillars.jsx` — what makes beava different (4 tiles)
- `Pipelines.jsx` — **new code-viewing component (2026-05-13).** Big vertical
  tabs on the left, each pipeline's outcomes + SDK code on the right. Auto-
  rotates every 8s; clicking any tab resets the timer. Outcome rows read
  `entity.feature = value > threshold` (mono) then the action in serif
  orange with a hand-drawn green dollar-gain marker (e.g. `+ $84 saved`).
- `PipelineShowcase.jsx` — horizontal tabs variant; kept for design comparison
- `CodeShowcase.jsx` — side-by-side "you write / you get" code block
- `Recipes.jsx` — recipe cards
- `FinalCTA.jsx` — three ways in
- `Footer.jsx` — 3-column with brand + links

`index.html` wires them into a realistic homepage.

## Implemented on the live site

`Pipelines.jsx` is implemented as a section in `beava-website/project/index.html`
(global `Pipelines` JSX function rendered after `<Pillars/>` and before
`<FinalCTA/>`). The hand-drawn "live, right now" marker + dollar-gain
markers on the live decision feed are also live there.
