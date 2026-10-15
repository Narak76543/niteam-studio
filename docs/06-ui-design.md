# 06 — UI Design

Style: **clean, modern, minimal, quiet.** Warm off-white surfaces, near-black type and
icons, hairline borders instead of shadows, lots of white space. Color is used only for
**status** and **one accent**. The content (images, script text, video) is the
colorful part — the chrome stays neutral.

Reference mood: modern SaaS workspace (light sidebar, big light-weight page titles,
white cards with hairline borders, small black icon tiles, outlined pill buttons).

## 1. Principles

1. **Neutral chrome, colorful content.** Scene images carry the color.
2. **Borders, not shadows.** 1 px hairlines; shadow only on popovers/modals.
3. **One primary action per screen** (solid black button). Everything else is outlined or ghost.
4. **Big calm titles, small quiet labels.** Large light page title, small muted metadata.
5. **Khmer first.** Every component must look right with Khmer text (taller line height).
6. **Fast on weak laptops.** No blur/backdrop-filter, no large animated gradients,
   animations ≤ 200 ms, transform/opacity only.

## 2. Tokens

Put these in `src/styles/tokens.css` and use them everywhere. **No raw hex values in components.**

```css
:root {
  /* surfaces */
  --bg-app:        #F3EFEA;  /* warm outer background */
  --bg-shell:      #FFFFFF;  /* main window panel */
  --bg-sidebar:    #F7F7F6;
  --bg-subtle:     #FAFAF9;  /* section wells (e.g. Shortcut area) */
  --bg-hover:      #F2F2F0;
  --bg-selected:   #FFFFFF;  /* selected nav item = white pill with border */

  /* borders */
  --border:        #E9E8E5;
  --border-strong: #D9D8D4;

  /* text */
  --text:          #141414;
  --text-muted:    #6B6B6B;
  --text-faint:    #9A9A97;
  --text-inverse:  #FFFFFF;

  /* ink (primary buttons, icon tiles) */
  --ink:           #141414;
  --ink-hover:     #2B2B2B;

  /* accent — used sparingly: progress, active waveform, focus ring */
  --accent:        #E8553E;  /* warm coral-red */
  --accent-soft:   #FDECE8;

  /* status */
  --ok:     #2F9E5B;  --ok-soft:     #E8F5EC;
  --warn:   #D98A1A;  --warn-soft:   #FCF3E3;
  --danger: #D64545;  --danger-soft: #FBEAEA;
  --info:   #3B6FD8;  --info-soft:   #EAF0FB;

  /* radius */
  --r-xs: 6px;  --r-sm: 8px;  --r-md: 12px;  --r-lg: 16px;  --r-xl: 22px;  --r-pill: 999px;

  /* spacing (4 px grid) */
  --s-1: 4px; --s-2: 8px; --s-3: 12px; --s-4: 16px; --s-5: 20px;
  --s-6: 24px; --s-8: 32px; --s-10: 40px; --s-12: 48px;

  /* shadows — popovers/modals only */
  --shadow-pop: 0 8px 24px rgba(20,20,20,.08), 0 1px 2px rgba(20,20,20,.06);

  /* motion */
  --ease: cubic-bezier(.2,.7,.2,1);
  --dur: 160ms;
}
```

**Light mode only** (owner's decision, 2026-10-15): there is no dark theme.

## 3. Typography

| Use | Font |
|---|---|
| Latin UI | **Manrope** (OFL) |
| Khmer UI + subtitles | **Kantumruy Pro** (OFL), fallback **Noto Sans Khmer** |

Bundle all fonts locally (`src/assets/fonts/`), never load from the internet.

```css
--font-ui: "Manrope", "Kantumruy Pro", "Noto Sans Khmer", system-ui, sans-serif;
--font-km: "Kantumruy Pro", "Noto Sans Khmer", "Manrope", sans-serif;
```

Set `:lang(km)` / `[lang="km"]` to use `--font-km` and the taller line heights.

| Token | Size / weight | Line height (Latin · Khmer) | Use |
|---|---|---|---|
| `display` | 40 / 400, letter-spacing −0.02em | 1.1 · 1.4 | Page title ("Projects", "Scenes") |
| `h2` | 20 / 500 | 1.3 · 1.6 | Section title ("Shortcut", "Render") |
| `h3` | 15 / 600 | 1.35 · 1.6 | Card title |
| `body` | 14 / 400 | 1.5 · 1.75 | Normal text, script editor |
| `small` | 12.5 / 400, `--text-muted` | 1.45 · 1.7 | Card description, metadata |
| `micro` | 11 / 500 | 1.3 · 1.5 | Pill buttons, badges, kbd hints |

## 4. Layout

```
┌ --bg-app (12 px padding around the window content) ───────────────────────┐
│ ┌ Sidebar 248 px ┐ ┌ Shell: --bg-shell, radius --r-xl, 1 px border ──────┐ │
│ │ project switch │ │ breadcrumb (small, muted)                            │ │
│ │ search  ⌘K     │ │ Page title (display)                  [search][btn]  │ │
│ │ ─ Essentials   │ │ content grid (gap 16) ............. │ right panel   │ │
│ │ ─ Projects     │ │                                      │ 320 px        │ │
│ │ ─ Support      │ │                                      │ (details)     │ │
│ └────────────────┘ └──────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────────────────┘
```

- Min window 1100×700; sidebar collapses to 64 px icons below 1280 px width.
- Page padding 24–32 px. Card grid gap 16 px.
- Right detail panel (scene details, render settings) slides in at 320 px; it does not cover content.

## 5. Components

**Sidebar nav item** — 36 px tall, radius `--r-sm`, icon 18 px + label. Hover `--bg-hover`.
Selected: `--bg-selected` + 1 px `--border` + very light `--shadow-pop`-free look (white pill).
Group headers ("Projects", "Support") are `small` muted with a chevron, `+` and drag handle on hover.

**Card** — `--bg-shell`, 1 px `--border`, radius `--r-md`, padding 16.
For "feature" cards use the *double hairline* look from the reference: outer 1 px
`--border` + inner 4 px `--bg-subtle` ring (`box-shadow: 0 0 0 4px var(--bg-subtle)` inside a
wrapper). Hover: border → `--border-strong`. No lift/shadow on hover.

**Icon tile** — 32×32, radius `--r-sm`, `--ink` background, white 18 px line icon.
Use for card headers (Projects page cards, empty states).

**Buttons**
| Variant | Style |
|---|---|
| Primary | `--ink` bg, white text, radius `--r-sm`, 36 px tall |
| Secondary | white bg, 1 px `--border-strong`, `--text` |
| Pill (small) | 24 px tall, `micro` text, 1 px `--border`, radius `--r-xs` (like "Connect") |
| Ghost / icon | transparent, 32×32, hover `--bg-hover` |
| Danger | text `--danger`, only inside confirm dialogs |

**Input / Search** — 40 px, white, 1 px `--border`, radius `--r-md`, leading search icon,
trailing kbd hint (`Ctrl K`) in `micro` `--text-faint`. Focus: border `--text` + 3 px `--accent-soft` ring.

**Status badge** — `micro`, radius `--r-pill`, soft bg + strong text:
`pending` neutral · `running` info + small spinner · `done` ok · `failed` danger · `stale` warn.

**Progress bar** — 4 px tall, radius pill, track `--border`, fill `--ink`
(use `--accent` only for the active render).

**Scene card** (Scenes page grid, 4 columns at 1440 px)
```
┌──────────────────────────┐
│  16:9 image, radius 8     │  ← placeholder: --bg-subtle + faint icon while pending
├──────────────────────────┤
│ 012 · 0:07     [● done]  │  micro, muted · status badge
│ Khmer narration, 2 lines │  body, clamp 2 lines
│ ▶  ⟳ voice  ⟳ image  ⋯  │  ghost icon buttons
└──────────────────────────┘
```

**Folder shortcuts** — the grey folder icons in the reference map to our **style presets**
or **recent projects**: 64 px illustration/thumbnail + 2-line `small` label, centered.

**Charts** (Render stats, usage) — thin 1.5 px lines, `--accent` for the main series,
dotted `--border-strong` for comparison, ring markers. Keep to at most one chart per page.

**Empty states** — icon tile + `h3` + one `small` line + one primary button.

**Toasts** — bottom-right, white, `--shadow-pop`, 1 px border, auto-hide 4 s.

## 6. Icons

**Lucide** (`lucide-react`), stroke 1.5, size 18 (nav) / 16 (buttons). Never mix icon sets.

## 7. Page mapping

| Page | Main content | Primary action |
|---|---|---|
| Projects (home) | Feature cards for recent projects (thumbnail, title, status, length), "Style presets" shortcut row | New project |
| New project wizard | Single centered card, max 640 px, 3 steps with a thin stepper | Generate script |
| Script | Two-column: scene list (left) + editor (right); narration vs spoken text tabs | Approve script |
| Scenes | Scene card grid + right panel for the selected scene | Generate all / Continue |
| Render (editor) | CapCut-style, light like the rest of the app (only the video frame is black): top bar with **Export**; centre live preview (canvas: image pan + voice + subtitles with word tracker, play/pause, prev/next scene, Space / ← →); right panel (subtitle font, word tracker on/off + colour swatches, current scene); bottom timeline (ruler, video thumbnails, subtitle cues, voice blocks, draggable playhead, zoom). Export dialog: settings → progress with % and ETA → Open video / Show in folder | Export |
| Settings | Grouped form rows, API key field masked with Test/Clear pills | — |

## 8. Accessibility

- Text contrast ≥ 4.5:1 (`--text-muted` on white passes; never use `--text-faint` for body text).
- Visible focus ring on every interactive element.
- All icon buttons have `aria-label` (from i18n).
- Hit targets ≥ 32 px.
