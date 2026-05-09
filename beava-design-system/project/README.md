# beava Design System

Design system for **beava** (beava.dev) — an open-source single-binary feature server for stream processing. Use this repo when designing any beava-branded surface: landing page, docs, blog, social cards, talks, swag.

> Positioning, in one line: **Redis with opinions for real-time features** — cozy, HTTP-first, single-binary. Sits deliberately apart from the Confluent / Flink / Materialize "streaming priesthood."

---

## What's in this repo

```
/
├── README.md                 ← you are here
├── SKILL.md                  ← Agent Skill entrypoint (portable to Claude Code)
├── colors_and_type.css       ← canonical design tokens (CSS variables)
├── assets/                   ← logos, mascot poses, illustrations
├── preview/                  ← design-system preview cards (rendered in Design System tab)
├── ui_kits/
│   ├── marketing/            ← beava.dev landing-page kit
│   ├── docs/                 ← beava.dev/docs kit
│   └── learn/                ← beava.dev/learn field-guide blog kit
└── uploads/                  ← original source files as received
```

Start by reading **`colors_and_type.css`** — it's the source of truth for tokens. Then open `ui_kits/*/index.html` to see the components in context.

---

## Source materials (received)

| File | What it is |
|---|---|
| `uploads/branding.png` | Primary beaver mascot, full-body, friendly pose. The default mark. |
| `uploads/Beaver Mascot Pose 2.svg` | Mascot pose variant (wave/greeting) |
| `uploads/Beaver Mascot Pose 3.png/.svg` | Mascot pose variant (action) |
| `uploads/Beaver Mascot Work Pose.svg` | "Working beaver" with log pile — use for docs/tutorials |
| `uploads/ChatGPT Image Apr 15 2026 Mascot Feedback (1).png` | Geometric/flat mark exploration — use as small-scale icon/favicon |

No codebase, Figma link, or slide deck was attached; the design system was derived from the mascot artwork + the written brand description. **If a codebase or Figma exists, attach it and we'll reconcile.**

---

# Content Fundamentals

beava's voice sits in the triangle between **Julia Evans** (warm, curious, pedagogical), **Patrick McKenzie** (precise, concrete, numerate), and **DuckDB** (indie-OSS, mascot-forward, a bit cheeky). Everything we write should feel like it was written by a specific human who ships.

### Tone
- **Warm, not cute.** The beaver is a mascot, not a brand personality we roleplay. We don't write *in character*.
- **Craft-oriented.** Sentences are a little longer than average. Commas are fine. Semicolons are allowed. We read our stuff out loud before shipping.
- **Self-aware.** We know we're entering a crowded space. We say why we made different choices, and we're honest about the tradeoffs.
- **Playful at the edges.** Mascot moments, well-placed em-dashes, a dry aside — yes. Pun headlines, emoji showers, Slack-speak — no.

### Person + address
- **"We"** for the project and team. (`We built beava because…`)
- **"You"** for the reader. (`You can run beava with one binary.`)
- **"I"** is fine in blog posts signed by a specific author. Avoid it in docs/product copy.
- Docs are **imperative or descriptive** (`Install with…`, `beava exposes…`), not "let's" or "we'll."

### Casing + punctuation
- **Sentence case** for everything: headings, buttons, nav labels, card titles. Never Title Case.
  - ✅ `Rolling counters without Kafka`
  - ❌ `Rolling Counters Without Kafka`
- `beava` is always capitalized; `beava` only appears inside code, URLs, or CLI output.
- Oxford comma, yes.
- Em-dash — spaced — is house style (not "—unspaced—").
- Numbers: write `1`, `2`, `3` (not "one, two, three") even in body copy, because we're a data product.

### Length + rhythm
- Landing-page headlines: **6–10 words**, one concrete promise.
- Subheads: **one sentence**, state what beava *is* before what it *does*.
- Button labels: **1–3 words**, verb first (`Get started`, `Read the docs`, `Star on GitHub`).
- Docs page intros: **≤3 sentences** before the first code block. Show code early.

### Emoji, unicode, fun characters
- **No emoji in product UI or docs.** It reads as "startup landing page from 2019."
- Emoji is acceptable in **blog posts** and **Discord** where a human is speaking.
- Unicode symbols like `→` and `·` and `✓` are welcome in UI — they feel typographic, not decorative.
- The **🦫 emoji** is fine in casual places (tweets, Discord) but we prefer the actual mascot artwork.

### Example specimens

**Landing hero:**
> **Real-time features without the streaming priesthood.**
> beava is a single-binary feature server for rolling counters, velocities, last-N-seen, leaderboards, and rate limits. HTTP in, HTTP out. No Kafka to babysit.
> `[ Get started ]  [ Star on GitHub · 8.2k ]`

**Docs page intro:**
> Rolling counters answer the question "how many of X have happened in the last N seconds?" — a useful primitive for everything from rate limits to trending-item lists. This page shows how to define one, how to query it, and how it performs under load.

**Blog (field guide) opener:**
> I spent a weekend trying to detect credit-card fraud with nothing but a CSV of transactions and a Postgres instance. This is what I learned, where it broke, and how a single rolling-counter operator in beava replaces about 180 lines of window-function SQL.

**Anti-patterns we don't do:**
- ❌ "Excited to announce the GA of our enterprise-ready stream processing platform."
- ❌ "Revolutionize your data pipeline with AI-powered real-time intelligence."
- ❌ "🚀 beava v2 is HERE!! 🎉"

---

# Visual Foundations

### Palette — warm, earthy, low-saturation

The palette is **three warm hues on a cream page**: burnt orange (the beaver's fur), deep brown (ink, tail, outline), and cream (paper). Everything else is a muted neighbor of these.

- **Backgrounds** are `#fdfaf4` cream by default. Secondary surfaces shift to `#f6eed9` (deeper cream). Code and wells use `#fbf5e8`. We **never** put UI on pure white `#ffffff` at page level — it feels clinical and breaks the warmth.
- **Text** is `#1a1714` (warm brown-black), not `#000`. Secondary text is `#8a6a54`.
- **Accent** is `#b85c20` (burnt orange) for all interactive affordances: links, primary buttons, active nav, progress. Hover darkens toward `#a04e16`. We use orange **sparingly** — one or two hits per viewport.
- **Rules** are `#e6dccb` — warm paper-tone divider, never cool gray.

### Imagery — hand-drawn, not stock

- **Illustration style:** Studio-Ghibli-adjacent, rounded, thick outlines, flat fills with minimal shading. The beaver mascot sets the rule: everything that ships as an illustration should look like it belongs next to it.
- **No stock photography.** Ever. beava pages have zero photographic content.
- **No 3D renders, no gradient orbs, no glassmorphism.**
- Backgrounds are **flat cream**. Occasionally we use a very subtle paper-grain noise (2–4% opacity) on hero sections — optional, not required.
- Blog-post headers use **hand-drawn-adjacent illustrations**, one per chapter, scene-based (the beaver building something, carrying logs, looking at a leaderboard). Flag to user: these need to be commissioned or drawn per post; we do not generate them programmatically.

### Typography — serif for headlines, sans for everything else

- **Instrument Serif** for display headlines and blog titles. A contemporary display serif with wide apertures and friendly terminals — reads less "editorial magazine," more "indie OSS by humans." Italics do real work — emphasize one or two words and color them with `var(--accent)`.
- **Inter Tight** for UI, body, nav, buttons, forms. Slightly tighter metrics than plain Inter; sits nicer next to the display serif. It's boring on purpose — the serif and the accent font get the attention.
- **JetBrains Mono** for code. Chosen over Fira/SF Mono because its ligatures are opt-out and its italic is tasteful.
- **Gaegu** for hand-drawn accents — marginal notes, chapter numerals, handwritten arrows, the occasional playful wink next to a serif heading. **Strict usage:** never body, never nav, never form labels, never legal/pricing, never strings longer than ~8 words. Always in `var(--accent)`, ideally with a micro-rotate (`transform: rotate(-2deg)`) and sitting NEXT TO a serif or mono element, not floating alone.
- Body text is **16px**, line-height **1.5** for UI and **1.7** for longform prose. Headings use `letter-spacing: -0.02em` at display sizes.
- We do NOT use all-caps anywhere except **eyebrow labels**, which are 12px Inter Tight 600, `letter-spacing: 0.08em`, orange.

### Spacing + layout

- **4px baseline.** All spacing tokens are multiples of 4.
- **Container widths:** `1200px` default, `720px` for prose (blog/docs body), `1280px` for docs app with sidebar.
- Layouts are **single-column-by-default** even on desktop — we drop into 2-column only for obvious splits (docs TOC, marketing feature grid).
- Generous whitespace. Sections get `80–128px` vertical padding on desktop. Density is earned, not a default.

### Corner radii

- Buttons and inputs: **10px** (`--r-md`).
- Cards: **14px** (`--r-lg`).
- Wide surfaces (hero cards, modals): **20px** (`--r-xl`).
- Capsule pills (badges, "Live" indicator): **999px** (`--r-pill`).
- We **do not** use sharp 0px corners, and we **do not** use 2xl+ radii on small elements (feels toy-like).

### Cards

A beava card has three layers:
1. Surface: `#ffffff` or `#fbf5e8` (cream paper).
2. Border: `1px solid #e6dccb` — always present. We lean on the border, not the shadow.
3. Shadow: `--shadow-sm` by default, `--shadow-md` on hover. Shadows are brown-tinted (`rgba(26,23,20,x)`), never cool gray.

For playful/sticker moments (capability tiles, blog chapter cards), we use `--shadow-pop` — a chunky flat drop that looks like a sticker.

### Borders

- Always **1px** except in code wells (inset-only) and hero dividers (2px).
- Default color: `#e6dccb`. Hover/focus: `#b85c20`.
- No dashed or dotted borders.

### Shadows

- Three elevation tiers: `--shadow-xs` (subtle), `--shadow-sm` (resting card), `--shadow-md` (hover/popover), `--shadow-lg` (modal). All warm-tinted.
- Inset shadow `--shadow-inset` on code blocks gives them a "pressed into paper" feel.
- **Never** use blue or gray-blue shadows. Never use colored shadows (orange glow, etc.).

### Hover + press states

- **Hover:** darken by ~8% and/or shift shadow up one tier. Nav links get an **underline** on hover, never a background change.
- **Press (`:active`):** shrink by 1px (translate-y 1px) and drop shadow by one tier. Never scale down — translateY only.
- **Focus:** 3px `rgba(184, 92, 32, 0.27)` ring, 2px offset. Always visible, never removed.
- Transitions: `200ms` default, `120ms` for micro-interactions, cubic-bezier `(0.22, 1, 0.36, 1)` (ease-out). The only spring we allow is on mascot-driven moments (hero illustrations, 404 page, Feed-the-beaver widget).

### Animation vocabulary

- **Fade + translateY(8px)** is the default entrance. 200ms, ease-out.
- **No parallax**, no scroll-linked zooms, no hero reveal choreography. The page loads, the page looks correct, the user reads.
- **Mascot moments:** the beaver can bounce (translateY with spring easing), blink, wag its tail on hover. These are the ONLY places we use spring easing.
- Reduce-motion honored globally (`@media (prefers-reduced-motion: reduce)`).

### Transparency + blur

- Blur is **almost never** used. The one acceptable place: sticky nav bar gets `backdrop-filter: blur(10px)` with a `rgba(253,250,244,0.85)` background when the page has scrolled past the hero.
- Opacity is used on washes and disabled states only — never as a hover effect.

### Background + imagery vibe

- **Warm, golden-hour, not saturated.** If we ever show a photo or render, it leans orange/brown/cream.
- No cold blues, no neon, no purple gradients.
- The only "pattern" allowed is a very subtle 2% paper grain (optional) and the mascot artwork at low opacity as a decorative element in empty states / 404 page.

### Fixed elements + layout rules

- **Nav bar**: 64px tall, sticky at top, cream bg that gets a subtle border-bottom on scroll.
- **Footer**: on cream-deep bg, 3 columns desktop / stacked mobile.
- **Docs sidebar**: 280px wide, sticky, independent scroll.
- **Max line length** for prose: ~68ch. Beyond that we pad.

---

# Iconography

beava does not have a custom icon library yet. Our approach:

- **Primary icon system: Lucide (`lucide.dev`)** — linked from CDN. Chosen for its consistent 1.5px stroke weight, rounded joins, and warm feel (vs. Heroicons' tighter geometry). All UI icons in the marketing site, docs, and blog should come from Lucide.
- **Stroke weight:** default `1.75` for 20px icons, `2.0` for 16px. We increase weight slightly at small sizes so icons don't disappear on cream.
- **Icon color:** inherits `currentColor`. In nav/meta, that's `--fg3` (`#8a6a54`). In CTAs or active states, it's `--accent` (`#b85c20`).
- **Size scale:** 14, 16, 20, 24. We don't use larger icons — that space goes to illustration.
- **Custom marks** (the beaver, the logo lockup, chapter illustrations) are **hand-drawn SVGs**, not in the icon system. They live in `assets/`.
- **Emoji** is not used as icons in the product. Acceptable in blog body prose and Discord.
- **Unicode typographic chars** (`→`, `·`, `✓`, `—`, `…`) ARE used deliberately, especially `→` after button labels and `·` as meta-dividers. They feel editorial and don't require loading an icon.

### Available mascot poses (in `assets/`)

| File | When to use |
|---|---|
| `logo-mark.png` | **Default mascot.** Full-body, friendly, hand-up. Use on marketing, 404, empty states, social cards. |
| `mascot-pose-2.svg` | Alt pose (greeting). Use as a secondary hero, in docs sidebars, or as an 'announce' moment. |
| `mascot-pose-3.png` / `.svg` | Action pose. Good for call-to-action moments, Feed-the-beaver widget. |
| `mascot-work-pose.svg` | **Working beaver with log pile.** Default illustration for docs, tutorials, and blog field-guide chapters — anything pedagogical. |
| `mascot-mark-geometric.png` | Flat/geometric mark. Use as **favicon** and at small scales (≤32px) where the detailed mascot would get muddy. |

---

# Interactive blog patterns

Blog / guide posts in this design system are **interactive build-alongs**, not static read-throughs. We have a fixed vocabulary of patterns. Use them in this order; don't invent new ones without a reason.

### Page width rule (critical)

One post has **two widths only**:
- **prose column** — `780px` max, centered with `40px` inner padding (inside an `860px` outer). Use for paragraphs, headings, ask-Claude rows, simple code blocks.
- **wide column** — `860px` max, centered. Use for the push-event form, live feed, entity dashboard, request/response pair, cost estimator, `ff-note`.

**Prose and wide share the same outer container** (`1040px`, centered). This way a wide block's left and right edges always align with the prose column's edges — no visual jump when scrolling between them.

Do NOT let widgets float inside prose at prose width and then expand on the next section — if a thing is interactive (a form, a dashboard, a terminal pair), it gets the wide width, always.

### Post header (`comp-post-header.html`)

Crumbs (`Guide / Chapter 1`) → orange eyebrow (`CHAPTER 1 · INTERACTIVE`) → large serif title (`60–68px`, max 16–18ch) → subtitle → mascot on the right (150–180px, pose 3). Use this header for every chapter-style / long-form tutorial post.

### Run-for-real callout (`comp-run-for-real.html`)

Cream card, orange eyebrow (`SKIP THE BROWSER — RUN IT FOR REAL`), one paragraph, dark terminal block with `docker` + `brew` + `curl` verify, meta strip below. Place it immediately after the header — this is the one place we tell readers that the browser sim is not the real thing. Use at most **once per post**.

### Section marker

`Part N · Label` eyebrow (orange, uppercase, 12px, 0.12em tracking) above an `h2` (40px serif). Parts are numbered starting at 1. Part 4 is conventionally the cost/scale section if the post has one.

### Side-by-side code compare

Two code blocks separated by a large rotated `≈` (Gaegu, orange). Pattern: `pandas (batch) ≈ beava (live)`. Labelled with small captions and a centered italic caption below explaining the difference. See `comp-code-compare.html`.

### Ask-Claude row

Orange-wash pill-row: orange rotated avatar with `?` + bold Gaegu "Or just ask Claude" title + mono command prompt underneath + white `Install skill →` CTA on the right. Use after each big pipeline code block to offer the skill alternative. Tone: the skill is the faster path, not a replacement.

### Action + status-pill row

Orange primary button (`>_ Register the pipeline`) + a green `✓ registered` pill to its right. The button state flips the pill. See `comp-action-pill.html`.

### Push-event form (`.pusher`)

White card with a 3px orange left border. Header row: `PUSH EVENT → @bv.table UserStats` tag on the left, orange `✦ Send a random event` button on the right. "or craft your own" italic note. Four-field grid: `user_id`, `path`, `category (derived, readonly, orange)`, `Push →` button. Width = wide.

### Live feed + response rows (`comp-live-feed.html`)

Left rail: `recent events` with pulsing green `live` dot and a scrolling list of event cards (uid + path + meta). Right panel: clean rows of `uid | value | label`. Below both: status strip with `✓ 200 OK · Xms` pill, pulsing `auto-refresh · 1.5s` pill, orange `>_ Re-run now` button.

### Live entity dashboard (`comp-entity-dashboard.html`)

4 KPI stat cards across the top + the events rail on the left + a 3-column grid of per-entity cards on the right. Each entity card: colored avatar badge (warm palette), uid, last-seen, big `views_total` serif number, mini bar for `views_24h`, category pills, activity sparkline. Use this AFTER the push form when the post's whole goal is "render live state."

### Request/response terminal pair

Side-by-side code blocks: `1. request — copy-paste this into your terminal` (curl) and `2. response — live, Xms` (JSON). Width = wide.

### Cost / RAM estimator (`comp-cost-estimator.html`)

Tabbed scale picker across the top (`10K / 100K / 1M / 10M / 100M users`) → three headline stats (entities / memory footprint / AWS fit-and-cost) → per-user byte breakdown table → footnote explaining why events don't add memory. Always goes near the end of the post. Drives the "this will actually fit on a t3.small" moment.

### End-of-post celebration (GREEN, not orange)

Dashed green border, `beava-success-wash` background, Gaegu scribble (`← you shipped it`), large serif title with one word in green italics, subtitle, two CTAs (green solid + green-outline), mascot on the right. **This is the one place we use green prominently** — it marks the "you finished" moment and visually differentiates from the orange "look at this" moments throughout the body.

### Next-posts pair

Two white cards side-by-side at the very bottom: small uppercase kicker (`NEXT CHAPTER` / `RECIPE`) + serif title. Always exactly two. Hover darkens border to orange.

### Ordering

For a typical chapter-style post:

1. Post header
2. Run-for-real callout (once)
3. Part 1 · concept — prose + code-compare
4. Part 2 · first pipeline — code block + ask-Claude + action-pill + pusher + live feed + request/response
5. Part 3 · real build — same sequence, richer table + entity dashboard
6. Part 4 · cost at scale — cost estimator + ask-Claude
7. End-of-post celebration
8. Next-posts pair

---

# Guidebook (chapter index) patterns

The guidebook is the landing page for `beava.dev/learn` — the jumping-off point for every chapter. It has its own distinct patterns separate from inside-a-post chrome.

### Guidebook hero (`comp-guidebook-hero.html`)

The top of the learn index. Orange eyebrow (`THE REAL-TIME FEATURE GUIDEBOOK`, 0.16em tracking) → very large serif title (`66px+`, max 16ch, often running 4 lines tall — let it wrap) → 3–4 sentence subtitle → `your progress` bar (4px track, orange fill on cream well) with `N / 6 chapters` count right-aligned → mascot (pose 3) on the right at 200px.

**Rules:** the title is the showpiece — don't cap it below ~4em font-size. Keep the subtitle concrete about what each chapter covers (name 3–5 of them) so the reader sees the destination before they click. Progress bar is persisted per-reader from localStorage — show it even on first visit, zero-filled.

### Chapter card (`comp-chapter-card.html`)

One card per chapter, stacked vertically (not a grid). Each card:
- `Chapter N` in Gaegu (orange, rotated `-2deg`, 22px) + orange `→` arrow. The arrow slides right on hover.
- Serif chapter title (28px, max 22ch).
- One-sentence meta about what you build.
- Tag row: `● interactive` (green pill), time estimate (`10 min`, orange pill), category tag (`basics` / `recipe` / `concept`, neutral pill).
- Mascot (pose 3 or work-pose) peeking from the right edge at 140px, anchored right-center so it looks like it's climbing out of the card.
- Cream `beava-paper` background, 18px radius, 1px border. Hover: border turns orange, card lifts 1px with a soft orange-tinted shadow.
- Locked / coming-soon chapters use `opacity: 0.55`, no hover transform, grayed-out arrow. Still visible in the list so readers see what's planned.

**Rules:** always stack, never grid. The long-scroll rhythm matters — readers should feel they can read down the list top to bottom without context-switching between columns. Alternate mascot poses between cards so the column feels alive (pose-3, work, pose-3, work).

---

## For designers + agents

- Read `SKILL.md` for the short version.
- Start from `colors_and_type.css` — don't invent new tokens. If a design needs a color that isn't there, flag it.
- Copy assets out of `assets/` — never reference images from `uploads/`.
- When in doubt: warmer, smaller, more restrained.

## Known caveats / to-iterate

- **Font substitution:** all faces are Google Fonts. If beava licenses custom faces, replace `--font-serif` and/or `--font-accent`. Gaegu in particular is a substitute stand-in for a proper custom hand-drawn beaver-adjacent script — revisit when budget allows.
- **Chapter illustrations are commissioned, not programmatic.** The design system expects them to exist as PNGs/SVGs in `assets/illustrations/` when they arrive.
- **No codebase / Figma was attached.** UI-kit components are designed from the brand description + mascot artwork, not reconciled against production code. If production exists, attach it and we'll reconcile component-by-component.
- **Dark mode** is not yet defined. The cream aesthetic is core; dark mode would need deliberate translation, not an algorithmic invert.
