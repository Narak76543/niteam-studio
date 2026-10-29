# 06 — UI Design

Style: **polished studio** (owner, 2026-10-29; replaces the warm-beige "quiet" look).
Cool neutral greys, near-black type, hairline borders with soft static shadows on raised
surfaces, and one brand colour taken from the app icon: **charcoal** (primary buttons, icon
tiles) and **coral** (the play button: focus ring, selected nav icon, active step, progress,
selection). The content (images, script text, video) is the colourful part.

Reference mood: Linear / Notion / Descript desktop: flush light sidebar, compact bold page
titles, white work area, cards with a full-bleed cover image.

## 1. Principles

1. **Neutral chrome, colourful content.** Scene images carry the colour; coral only marks
   "you are here / this is active / this has focus".
2. **Hairlines first, soft shadows second.** 1 px borders everywhere; `--shadow-xs` on raised
   controls (buttons, inputs, cards, selected pills), `--shadow-md` on card hover,
   `--shadow-pop` on popovers/modals. Shadows are static (no blur filters).
3. **One primary action per screen** (solid charcoal button). Everything else is outlined or ghost.
4. **Compact bold titles, small quiet labels.** 26 px / 700 page title, muted metadata.
5. **Show status only when it needs attention.** A finished project has no badge; paused,
   generating, review and failed states do.
6. **Khmer first.** Every component must look right with Khmer text (taller line height;
   no uppercase or letter-spacing on Khmer labels).
7. **Fast on weak laptops.** No blur/backdrop-filter, no large animated gradients,
   entrance animations <= 240 ms, transform/opacity only; loops only while loading.

## 2. Tokens

All tokens live in `src/styles/tokens.css` (the file is the source of truth; **no raw hex
values in components or `base.css`**). Groups:

| Group | Tokens |
|---|---|
| Surfaces | `--bg-app` (window, behind the sidebar), `--bg-shell` (work area, cards, inputs), `--bg-sidebar`, `--bg-subtle` (wells, placeholders), `--bg-hover`, `--bg-selected`, `--bg-inset` (segmented track) |
| Borders | `--border`, `--border-strong` |
| Text | `--text`, `--text-muted`, `--text-faint`, `--text-inverse` |
| Brand | `--ink` / `--ink-hover` (charcoal), `--accent` (coral), `--accent-strong` (coral as text), `--accent-soft`, `--accent-ring` (focus/selection ring) |
| Status | `--ok`, `--warn`, `--danger`, `--info` + `-soft` backgrounds |
| Over images | `--scrim`, `--scrim-text` (scene number, aspect chip, neutral badges on thumbnails) |
| Elevation | `--shadow-xs`, `--shadow-sm`, `--shadow-md`, `--shadow-pop`, `--backdrop`, `--btn-highlight` |
| Shape / space | `--r-xs` 6 / `--r-sm` 8 / `--r-md` 10 / `--r-lg` 14 / `--r-xl` 18 / `--r-pill`; spacing `--s-1` to `--s-12` on a 4 px grid |
| Layout | `--sidebar-w` 240 px, `--topbar-h` 56 px |
| Motion | `--ease`, `--dur` 140 ms |

**Light and dark mode** (dark added at the owner's request, 2026-10-17; the 2026-10-15
"light only" decision is replaced). Settings → Appearance: *Same as Windows* (default,
follows the Windows app mode live) / *Light* / *Dark*, stored as `settings.theme`
(`null` | `"light"` | `"dark"`). `src/lib/theme.ts` sets `data-theme` on `<html>`; the dark
values are the `:root[data-theme="dark"]` block in `tokens.css` (timeline cue/voice and text
diff colours are tokens too). The video frame stays black in both.

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
| `display` | 26 / 700, letter-spacing −0.02em (Khmer: 600, no spacing) | 1.2 · 1.5 | Page title ("Projects", "Scenes") |
| `h2` | 16 / 650 | 1.35 · 1.6 | Section title ("Recent projects", chapter titles) |
| `h3` | 14 / 600 | 1.4 · 1.6 | Card title |
| `body` | 13.5 / 400 | 1.5 · 1.75 | Normal text, script editor |
| `small` | 12.5 / 400, `--text-muted` | 1.45 · 1.7 | Card description, metadata |
| `micro` | 11 / 500 | 1.3 · 1.5 | Pill buttons, badges, kbd hints |

## 4. Layout

```
+ Sidebar 240 px (--bg-sidebar, 1 px right border) + Work area (--bg-shell, flush) ----------+
| [icon] Nitean Studio                              | breadcrumb (small, faint)                |
| [ Search projects        Ctrl K ]                 | Page title (display)     [search][btn]   |
| ESSENTIALS                                        | ------------------- hairline ----------- |
|  Projects / New project / Settings                | content (max 1480 px, centred)           |
| RECENT                                            |                                          |
|  5 latest projects                                |                                          |
| -------                                           |                                          |
|  language toggle                                  |                                          |
+---------------------------------------------------+------------------------------------------+
```

- No outer gutter or floating panel: the sidebar sits on the window background and the
  work area fills the rest.
- Min window 1100x700; sidebar collapses to 64 px icons below 1280 px width and always in
  the video editor.
- Page padding 24-32 px. Card grid gap 20 px. The page header ends with a hairline.

## 5. Components

**Sidebar** — the brand row uses the real app icon (`src/assets/brand/app-icon.png`, a copy of
`src-tauri/icons/128x128.png`). Search is a field-like button with a `Ctrl K` key cap. Nav item:
34 px, radius `--r-sm`, icon 18 px + label in `--text-muted`. Hover `--bg-hover`. Selected:
white pill (`--bg-selected` + 1 px `--border` + `--shadow-xs`), label `--text` 600, icon
`--accent`. Group headers are 11 px uppercase `--text-faint` (Khmer: no uppercase). The
language toggle sits in a footer above a hairline.

**Card** — `--bg-shell`, 1 px `--border`, radius `--r-lg`, padding 16, `--shadow-xs`.
**Feature card** (project / scene tiles): single hairline + `--shadow-xs`; the cover image runs
edge to edge across the top (no inner padding, hairline under it); hover: `--border-strong` +
`--shadow-md` and a 2.5 % image zoom. Overlays on the cover use `--scrim`. Project cards
reveal the delete button over the cover's top-right corner on hover/focus only. Scene cards
end in an action bar (`--bg-subtle`, hairline above). Chapter headers on the Scenes page show
"Chapter n" as a coral chip before the title.

**Icon tile** — 36×36, radius `--r-md`, `--ink` background, white 18 px line icon, `--shadow-sm`.
Use for card headers (Projects page cards, empty states).

**Buttons**
| Variant | Style |
|---|---|
| Primary | `--ink` bg, `--text-inverse`, radius `--r-sm`, 34 px tall, weight 600, `--shadow-xs` + top sheen |
| Secondary | `--bg-shell`, 1 px `--border-strong`, `--text`, `--shadow-xs` |
| Pill (small) | 26 px tall, `micro` 600 text, 1 px `--border`, radius `--r-pill` |
| Ghost / icon | transparent, 32×32, hover `--bg-hover` |
| Danger | text `--danger`, only inside confirm dialogs |

**Input / Search** — 36 px, `--bg-shell`, 1 px `--border-strong`, radius `--r-sm`, `--shadow-xs`,
leading search icon, trailing kbd hint (`Ctrl K`) in `micro` `--text-faint`. Focus: border
`--accent` + 3 px `--accent-ring`. **Segmented control**: `--bg-inset` track sized to its
options, selected option white + `--shadow-xs`. Checkboxes, radios and sliders use `--accent`.

**Status badge** — `micro` 600, radius `--r-pill`, soft bg + strong text; project status
badges carry a 6 px status dot:
`pending` neutral · `running` info + small spinner · `done` ok · `failed` danger · `stale` warn.

**Progress bar** — 4 px tall, radius pill, track `--border`, fill `--accent`.

**Stepper** (wizard) — 3 px bars on `--bg-inset`; numbered circles: active coral, done charcoal.

**Tabs** — active tab `--text` 600 with a 2 px `--accent` underline.

**Selected list row** (Script scene list, Settings section list) — white pill + 3 px inset
`--accent` bar on the left.

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

**Empty states** — dashed `--border-strong` well on `--bg-subtle`, radius `--r-xl`: icon tile +
`h3` + one `small` line + one primary button.

**Toasts** — bottom-right, white, `--shadow-pop`, 1 px border, auto-hide 4 s.

**Motion** (`src/styles/motion.css`, 2026-10-29) — calm and quick, never in the way:

| What | Animation |
|---|---|
| Page change | page fades up 6 px, 220 ms (`rise-in`, ease-out) |
| Card grids (projects, scenes, presets) | same rise-in, staggered 28 ms per card, capped after the 12th |
| Thumbnails | image fades in when its file arrives (240 ms) |
| Notices, badges on images | rise-in when they appear |
| Update card / toasts / dialogs | pop-in / slide in from the right / `dialog-in` + backdrop fade |
| Wizard steps | step content slides in from the right; stepper bars and numbers cross-fade |
| Buttons | press scales to 0.97 (90 ms) |
| Loading a project (Scenes / Script / editor) | `PageSkeleton` in the page's shape, pulsing opacity (1.4 s loop, staggered) |
| App start | `BootSplash`: app icon breathing + thin coral bar |
| Compact "update available" dot | slow pulse |

All entrances use `animation-fill-mode: both` ending at `transform: none`, so nothing stays
in a transformed state (fixed-position dialogs inside pages keep working). With Windows
"Animation effects" off (`prefers-reduced-motion`), entrances and hover nudges are disabled
and skeletons pulse at half speed.

## 6. Icons

**Lucide** (`lucide-react`), stroke 1.5, size 18 (nav) / 16 (buttons). Never mix icon sets.

## 7. Page mapping

| Page | Main content | Primary action |
|---|---|---|
| Projects (home) | Feature cards for recent projects (thumbnail, title, status, length), "Style presets" shortcut row | New project |
| New project wizard | Single centered card, max 640 px, 3 steps with a thin stepper | Generate script |
| Script | Two-column: scene list (left) + editor (right); narration vs spoken text tabs | Approve script |
| Scenes | Scene card grid + right panel for the selected scene | Generate all / Continue |
| Render (editor) | CapCut-style, same theme as the rest of the app (only the video frame is black): top bar with **Export**; centre live preview (canvas: image pan + voice + subtitles with word tracker, play/pause, prev/next scene, Space / ← →); right panel (subtitle font, word tracker on/off + colour swatches, current scene); bottom timeline (ruler, video thumbnails, subtitle cues, voice blocks, draggable playhead, zoom). Export dialog: settings → progress with % and ETA → Open video / Show in folder | Export |
| Settings | Section list on the left (sticky, highlights the current section; a chip row below 1200 px) + grouped form rows, API key field masked with Test/Clear pills | — |

### Updates 2026-10-16

- **Projects cards** show the project's first scene image with the status badge over it.
- **Scenes page** groups cards under chapter headers (chapter number, title, scene count);
  scene number · length sit on the image, *Key* in its corner; voice/image badges only appear
  while something is not done. Header action **Improve image prompts** (03 Step 1c).
- **Script page**: the scene's current image sits next to its image prompt.
- **Video editor**: the app sidebar is icons-only so the preview and timeline get the width;
  timeline clips show a thumbnail down to 12 px wide.
- Long page titles wrap to two lines instead of being cut off.

## 8. Accessibility

- Text contrast ≥ 4.5:1 (`--text-muted` on white passes; never use `--text-faint` for body text).
- Visible focus ring on every interactive element.
- All icon buttons have `aria-label` (from i18n).
- Hit targets ≥ 32 px.
