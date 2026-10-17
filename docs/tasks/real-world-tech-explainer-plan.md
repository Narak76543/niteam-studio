# Plan: "Real-world Tech Explainer" (STEP 0 — plan only)

Owner brief, 2026-10-17. Status: **decisions made, spikes done (§6), waiting for the owner's OK
to build**. Nothing built yet.

## Owner decisions (2026-10-17)

1. Unsplash dropped for now.
2. CC BY-SA off by default; Settings switch explains the risk.
3. Pexels optional; the feature must fully work with **Pixabay + Openverse/Commons alone**.
4. CLAUDE.md Exception 5 added: Pexels, Pixabay, Openverse, Wikimedia Commons, this feature only.
5. Gemini auto-pick off by default; **local ranking** instead (keywords, resolution, license).
6. Resolution handling (§4a): pixel size on every candidate, "low-res" badge, bigger ranks
   higher, zoom limited on small images; docs explain Pixabay full-resolution access.

Goal: technical explainer videos whose scenes use real photos, stock video clips, diagrams,
code and the user's own screenshots, with AI images as a fallback only. It must stay free and
legally safe.

---

## 1. API terms, limits and license fields (checked 2026-10-17)

| Source | Key | Rate limit | Download & store? | Attribution | License fields |
|---|---|---|---|---|---|
| **Pexels** (photos + videos) | free key, `Authorization` header | 200 / hour, 20 000 / month; `X-Ratelimit-*` headers **only on 2xx** (reset = monthly UNIX time) | not restricted in the docs; no bulk copying; don't replicate Pexels | API rule: prominent link to Pexels where requests are made + "Photo by X on Pexels" when possible. License: credit optional | one license for everything (Pexels License); `photographer`, `photographer_url`, `url` (page), `src.*`; video: `user.name/url`, `duration`, `video_files[]` (w, h, quality, fps, link) |
| **Pixabay** (photos + videos) | free key, `key=` param | 100 / 60 s per key; `X-RateLimit-*` (reset = seconds) | **must download** (no permanent hotlinking); **search results must be cached 24 h**; no systematic mass downloads | show the source whenever results are shown. License: credit optional | Pixabay Content License; `user`, `pageURL`, `largeImageURL` (**max 1280 px** without full API access), `tags`; video: `videos.large/medium/small` (url, w, h, size), `duration` |
| **Openverse** (includes Wikimedia Commons, Flickr CC, rawpixel, museums…) | none needed (anonymous); optional registered client (OAuth client credentials) | **measured 2026-10-17: anonymous 20 / min burst, 200 / day sustained** (`x-ratelimit-limit-anon_burst`, `…-anon_sustained` + `…-available-…` headers); `page_size` ≤ 20 | yes, under each file's own license | per license (CC BY / BY-SA need credit); API returns a ready `attribution` text | `license` (`cc0`, `pdm`, `by`, `by-sa`, `by-nc`…), `license_version`, `license_url`, `creator`, `creator_url`, `foreign_landing_url`, `url`, `source`, `provider`, `width`, `height`. Filter with `license=cc0,pdm,by,by-sa`. **Images only, no video.** |
| **Wikimedia Commons API** (**searched directly**, see §6.3, and used to re-check licenses) | none; **descriptive User-Agent with contact required** (else HTTP 403) | no fixed number; serial requests, `maxlag`, back off on 429 | yes, under the file's license | per license | `imageinfo` + `extmetadata`: `LicenseShortName` (most reliable), `LicenseUrl`, `Artist` (HTML), `Credit`, `AttributionRequired`, `Copyrighted`, `Restrictions` (trademark, personality rights) |
| **Unsplash** | developer account | demo 50 / hour, production 1000 / hour | ⚠️ **"All API uses must use the hotlinked image URLs"** (API Terms §6, a "material breach" otherwise); download tracking call required per download | must credit Unsplash + photographer with a link to the profile (`?utm_source=…&utm_medium=referral`) | `user.name`, `user.links.html`, `links.html`, `urls.*`, `links.download_location` |

### Findings that change the brief

1. **Unsplash: leave it out (recommended).** Our renderer needs the file on disk (FFmpeg),
   which conflicts with the hotlinking rule. Their guidelines also say end users should not have
   to register for a developer account and credentials must not be shared, and we have no
   server to act as a proxy. Both point against BYOK in a desktop app. We can ask
   api@unsplash.com later and add it if they agree.
2. **Pexels: "New API key issuance is currently paused"** (pexels.com/api, today). New users
   may not be able to get a key. Keep Pexels optional: Pixabay + Openverse must work without it.
3. **Pixabay photos are at most 1280 px wide** with a normal key (`largeImageURL`); the
   original (`imageURL`), `fullHDURL` and `vectorURL` need **full API access**. They are marked
   "low-res" for 1080p (§4a). *How to apply (for the docs and Settings help):* log in at
   pixabay.com, open the API docs (pixabay.com/api/docs/) and use **"Request full API access"**
   in the Support section, or write via pixabay.com/service/contact/. Approval is per account
   (each user applies for their own key), and they ask that the API is "implemented properly"
   (cached searches, downloaded files, source shown). No published criteria or turnaround time.
4. **CC BY-SA:** a video that crops, pans and combines a BY-SA image may count as an
   *adaptation*, which would have to be shared under BY-SA too. **Recommended:** BY-SA off by
   default, with a Settings switch "Allow CC BY-SA (your video may have to be shared under the
   same license)". CC0, PD and CC BY are on. NC and ND are never allowed (as briefed).
5. **Openverse license data can be stale.** For Wikimedia results we re-check the license with
   the Commons API at download time. If they disagree, or `Restrictions` lists trademark or
   personality rights, we use the Commons value and show a warning.
6. **Pexels and Pixabay licenses** forbid showing identifiable people in a bad light, implying
   endorsement, and using trademarks or logos commercially. Credits are optional but we always
   add them. A one-line note in the picker covers this.
7. Stock footage is sometimes wrongly claimed by YouTube Content ID. The stored credits
   (source page URL + license) are what the user needs to dispute such a claim.
8. **CLAUDE.md needs a new exception** (owner's decision): downloads from Pexels, Pixabay,
   Openverse and Wikimedia Commons (not AI services; BYOK keys in the keyring; no scraping).

---

## 2. Data model and files

### Scene (`pipeline/project.rs`)

```rust
pub struct Scene {
    // … existing fields; `image_prompt` + `image` stay for the ai_image fallback
    #[serde(default)]
    pub visual: SceneVisual,          // old projects: kind = ai_image → nothing changes
}

pub enum VisualKind { AiImage /*default*/, RealPhoto, StockVideo, Diagram, Code, Screenshot }

pub struct SceneVisual {
    pub kind: VisualKind,
    pub keywords: Vec<String>,        // ENGLISH search terms (also for Khmer scripts)
    pub diagram: Option<DiagramSpec>, // { template: flow|sequence|timeline|architecture|comparison,
                                      //   mermaid: String, labels_lang: km|en }
    pub code: Option<CodeSpec>,       // { language, text, terminal: bool, type_out: bool }
    pub candidates: Vec<Candidate>,   // last search (thumbs in scenes/<id>/candidates/)
    pub asset: Option<String>,        // chosen AssetRecord id
    pub focus: Option<[f32; 2]>,      // subject centre 0..1 for the 16:9 crop
    pub auto_picked: bool,            // chosen by Gemini → "check" badge until the user confirms
    pub status: AssetStatus, pub error: Option<String>, pub error_kind: Option<String>,
    pub render_path: Option<String>,  // final still for photo / diagram / code / screenshot
}
```

### Asset registry (per project): `projects/<id>/assets/assets.json` plus the files next to it

```rust
pub struct AssetRecord {
    pub id: String,                   // "<source>_<source_id>"
    pub source: AssetSource,          // pexels | pixabay | openverse | wikimedia | user
    pub media: Media,                 // photo | video
    pub file: String,                 // assets/<id>.jpg|mp4
    pub title: String,
    pub author: String, pub author_url: Option<String>,
    pub license: String,              // "pexels" | "pixabay" | "cc0" | "pdm" | "by" | "by-sa" | "own"
    pub license_version: Option<String>, pub license_url: Option<String>,
    pub original_url: String,         // landing page (Pexels / Pixabay / Commons page)
    pub file_url: String,             // what was downloaded
    pub downloaded_at: String,        // RFC 3339
    pub width: u32, pub height: u32, pub duration_ms: Option<u64>,
    pub attribution: String,          // ready credit line
    pub restrictions: Vec<String>,    // from Commons
    pub sha256: String,
}
```

- Assets are shared by scenes, never downloaded twice, and kept across re-renders.
- Search cache: `%LOCALAPPDATA%\NiteanStudio\cache\stock\<source>\<sha(query+params)>.json`,
  24 h (Pixabay's rule; we use it for every source).
- `Project.stock_pause: Option<StockPause { source, resets_at_utc }>`, the same pattern as
  `ImagePause`.
- `hashing::clip_hash` adds the asset id, focus, kind and the diagram or code text.
- Credits: `credits::build(project) -> String`, grouped by source, one line per asset, e.g.
  `"Server rack" by Jane Doe — CC BY 4.0 — https://commons.wikimedia.org/…`, then
  `Photos and videos from Pexels and Pixabay.`

### Settings and keys

- Keyring users `pexels_api_key` and `pixabay_api_key`, stored like the Gemini key (never logged,
  never in project files).
- `settings.stock = { pexels: bool, pixabay: bool, openverse: bool, allow_by_sa: false,
  auto_pick: false }`.

### Files

**Rust, new:** `stock/mod.rs` (trait `StockSource`, `Candidate`), `stock/pexels.rs`,
`stock/pixabay.rs`, `stock/openverse.rs`, `stock/commons.rs` (search + license check,
User-Agent `NiteanStudio/<ver> (github.com/Narak76543/niteam-studio)`), `stock/license.rs`
(allow list, license-name parsing, credit text), `stock/rank.rs` (local ranking, §4a), `stock/cache.rs` (search cache, download, registry), `stock/rate.rs` (headers →
pause), `pipeline/visual_job.rs` (search → candidates → optional auto-pick → download;
resumable per scene), `gemini/visual_pick.rs` (vision auto-pick + subject box),
`commands/visuals.rs`, `pipeline/credits.rs`.

**Rust, changed:** `pipeline/project.rs`, `pipeline/presets.rs` (new preset
`tech_explainer`), `gemini/script.rs` (with this preset the schema adds `visual_type`,
`keywords`, `mermaid`, `code` and rules), `render/clip.rs` (video input, zoom motion, code
reveal overlays), `pipeline/render_job.rs`, `pipeline/hashing.rs`, `storage/secrets.rs`,
`storage/settings.rs`, `lib.rs`, `commands/mod.rs`.

**Frontend, new:** `lib/diagramRender.ts`, `lib/codeRender.ts`,
`components/VisualPanel.tsx` (type switch), `components/CandidatePicker.tsx`,
`components/DiagramEditor.tsx`, `components/CodeEditor.tsx`,
`components/ScreenshotDrop.tsx`, `components/StockSettingsSection.tsx`,
`components/CreditsPanel.tsx`.

**Frontend, changed:** `lib/api.ts`, `lib/types.ts`, `pages/ScenesPage.tsx`,
`pages/RenderPage.tsx` (Credits with Copy, next to the existing document credit),
`pages/SettingsPage.tsx`, i18n.

**Docs:** new `docs/11-real-world-visuals.md`; update 02, 03, 05 (new section), 06, 07,
`CLAUDE.md` (exception 5), `THIRD_PARTY_NOTICES.md` (mermaid, shiki, JetBrains Mono).

---

## 3. Renderers

### Diagrams: **Mermaid, bundled in the frontend, drawn in the WebView** (recommended)

- `mermaid` (MIT) is loaded only when needed (`import()`), so the start-up bundle doesn't grow.
  It runs in the WebView, so there's no Node and no headless Chrome. It is the same path as the
  subtitle PNGs (WebView canvas → PNG bytes → Rust saves).
- It covers flowchart, sequence, timeline and architecture/block diagrams. Gemini writes Mermaid
  well, and `mermaid.parse()` lets us validate it. If parsing fails: one Gemini repair request,
  then the user can edit it or switch the scene type.
- **Comparison** (A vs B) and "big number" scenes get **our own SVG templates**, because
  Mermaid has no good comparison layout. Two small templates, no general SVG engine.
- Styling: a Mermaid `themeVariables` set built from our tokens (preset palette, soft
  background), rendered at 1920×1080 with the subtitle band kept clear.
- **Khmer labels:** `htmlLabels: false` (plain SVG `<text>`; an SVG with `<foreignObject>`
  blocks canvas export). Kantumruy Pro is embedded in the SVG as an `@font-face` data URL so
  the rasterised image shapes Khmer correctly. Mermaid measures label width with the same font
  in the DOM.
- **Spike first:** Khmer label with subscripts (e.g. "ស្ថាបត្យកម្ម") → PNG, checked visually.

### Code: **Shiki (MIT) for tokens + our own canvas drawing**

- Shiki with the JavaScript regex engine (no WASM) and ~12 languages (py, js/ts, rust, go,
  java, c/cpp, bash, json, yaml, sql, html/css). We draw the tokens on a canvas ourselves: an
  editor-style or terminal-style card (dark, macOS-style dots or prompt `$`), at most ~18 lines
  at 1080p, long code is cut with a note.
- Font: **JetBrains Mono** (OFL), with Kantumruy Pro as fallback for Khmer comments and strings.
- **Typed out over time:** one PNG per revealed line (or per few characters for short snippets).
  FFmpeg overlays them at the voice timings using the same `enable='between(t,…)'` method as
  subtitles. Cheap on the laptop: no per-frame rendering.

### Motion

- Photos: gentle zoom (1.00 → up to 1.08) with **`zoompan`** on an input pre-scaled to about
  the output size × max zoom (spike §6.2: smoothest, +35 % render time; the `scale=eval=frame`
  variant stutters). Pan-only scenes keep today's `crop`. 03-pipeline.md's "never zoompan"
  rule gets an exception for this case, with the measurement.
- 16:9 crop that keeps the subject: `focus` comes from (a) the Gemini subject box when
  auto-pick ran, else (b) the photo centre; the user can drag the focus point in the editor. The
  crop is computed in Rust from focus + source size.
- Stock video: `-ss` start, trim to the scene length, scale + crop to 16:9 (cover, around
  focus), 30 fps, audio dropped. Prefer clips ≥ scene length. A shorter clip loops once with a
  0.5 s crossfade; if it's still short, the last frame is held (`tpad`). Downloads: the 1080p
  file (720p for 720p projects), never 4K.
- Diagram, code, screenshot: still image, very slow zoom (or none for code, so it stays
  readable). The subtitle band and the existing render pipeline are unchanged.

---

## 4. Scene editor UI (Scenes page, right panel)

```
┌ Scene 7 · 0:42–0:51 ──────────────────────────────────────────────┐
│ Visual  [Photo] [Video] [Diagram] [Code] [Screenshot] [AI image]  │
│                                                                   │
│ Search  [ data center server racks          ] [Search]            │
│ Sources  ● All  ○ Pexels  ○ Pixabay  ○ Openverse                  │
│ ┌──────┐ ┌──────┐ ┌──────┐                                        │
│ │ ✓    │ │      │ │  ▶0:12│   each card: thumbnail, source badge, │
│ └──────┘ └──────┘ └──────┘   license chip (Pexels / CC BY / CC0), │
│ ┌──────┐ ┌──────┐ ┌──────┐   author; video: duration + hover play;│
│ │      │ │ 720p │ │      │   "720p" when the file is small        │
│ └──────┘ └──────┘ └──────┘                                        │
│ [More results]  [Auto-pick with Gemini]                           │
│                                                                   │
│ Preview 16:9  (drag ⊕ to set the subject; subtitle band shaded)   │
│ ┌───────────────────────────────────────┐                         │
│ │                ⊕                      │                         │
│ │▒▒▒▒▒▒▒▒▒▒▒ subtitle band ▒▒▒▒▒▒▒▒▒▒▒▒▒│                         │
│ └───────────────────────────────────────┘                         │
│ Photo by Jane Doe on Pexels · Pexels License · Open page ↗        │
│ ⚠ Auto-picked — check it fits   [Looks good]                      │
│ Photos and videos provided by Pexels, Pixabay and Openverse ↗     │
└───────────────────────────────────────────────────────────────────┘
```

- 6 candidates per search, mixed across the enabled sources, ordered by the local ranking
  (§4a). Thumbnails come from the API URLs (temporary display is allowed). The full
  file is downloaded only when the user picks one (or confirms auto-pick).
- **Diagram:** Mermaid text (left) + live preview (right), template picker,
  [Fix with Gemini].
- **Code:** language, code text, [Editor | Terminal], [Type out ✓].
- **Screenshot:** drop zone/file picker + required checkbox "This is my own screenshot or I
  have permission to use it" (stored as license `own` + date).
- Scene grid cards get a small type icon. Scenes waiting for a pick or review show a badge, and
  the Render step is blocked until every scene has a visual (with an "AI image for the rest"
  button).
- **Auto-pick** (off by default; owner decision 5): one Gemini vision call per scene with the 6
  thumbnails → index + subject box + reason. Without it the top-ranked candidate is
  pre-selected but **not** downloaded until the user confirms. It uses the same quota pause as the script (the free tier
  allowed only ~20 requests/day on gemini-3.8-flash during the document test). The result is
  always marked for review.
- Rate limit hit → the scene shows "Pexels limit reached — continues at 14:00" and the visual
  job pauses and resumes like `paused_quota`. No key or account rotation.
- **Render page:** "Credits" panel with the generated block and a **Copy** button (same as the
  document credit today).

---

### 4a. Resolution handling and local ranking

- Each card shows the pixel size ("6000×4000"). **"Low-res" badge** when the image is smaller
  than the project frame (1920×1080 or 1280×720) after the 16:9 crop, i.e.
  `min(w, h·16/9) < frame width`. For videos: the best file ≤ frame size; "low-res" when below
  the frame.
- **Zoom limit:** max zoom = `min(1.08, cropped_width / frame_width)`. An image that already
  needs upscaling gets **no zoom** (pan only if there is room, else static). The panel says
  "Small image — will look soft" and suggests another candidate.
- **Local ranking** (no AI), sum of:
  - keyword match: share of the scene's English keywords found in title/tags/alt (0–40);
  - resolution: 0 when low-res, then up to 30 for ≥ 2× the frame width (log scale);
  - shape: landscape 10, square 4, portrait 0 (portrait crops lose the most);
  - license: CC0/PD/Pexels/Pixabay 10, CC BY 8, CC BY-SA 5 (when allowed);
  - penalty −50 for likely AI-generated files (Commons category "AI-generated images", or
    "DALL·E"/"Midjourney"/"Stable Diffusion" in the title), so they are effectively hidden;
  - tie-break: mix sources (no source fills the grid).
- Unit-tested pure function (`stock/rank.rs`).

## 5. Build order (after the OK)

Spikes (half a day, results back to the owner): Khmer Mermaid → PNG; zoom motion speed on
the laptop; a Pexels key test (if the owner can get one).

Then: 1) data model + Settings + Pixabay/Openverse/Commons clients + cache + license filter;
2) script preset + Gemini schema; 3) picker UI + downloads + credits; 4) render: photo motion,
stock video, screenshot; 5) diagrams; 6) code; 7) auto-pick; 8) Pexels (if keys can be had).

Questions 1–5 answered (see "Owner decisions" at the top).

## 6. Spike results (2026-10-17)

Files: the session scratchpad (`spike-mermaid/`, `spike-zoom/`, `spike-stock/`); not in git.

### 6.1 Khmer diagram → PNG ✅
Mermaid 11.17.2 in Edge headless (the same Chromium engine as WebView2), `htmlLabels: false`,
Kantumruy Pro embedded in the SVG as base64 `@font-face`, SVG → `Image` → canvas → PNG at
1920×1080 with the subtitle band kept clear. Flowchart, timeline and sequence diagrams with
Khmer + English labels: coeng/subscripts (ស្ថាបត្យកម្ម, មូលដ្ឋានទិន្នន័យ), vowels and
signs are shaped correctly, and the canvas export is not blocked. `mermaid.parse()` rejects
broken input. To fix when building: embed the **bold** weight too (the timeline title fell back
to another font), and timeline colours need their own `themeVariables` (`cScale*`).

### 6.2 Zoom speed and smoothness (owner's laptop: RTX 4050 Laptop) ✅
12 s clip from a 2150×1210 scene image, 1080p, same settings as `clip.rs`:

| Motion | libx264 veryfast | h264_nvenc | stalled frames* |
|---|---|---|---|
| current pan (`crop`) | 2.0 s | 1.7 s | 18 / 319 |
| zoom, `scale=eval=frame` + `crop` | 2.7 s | 2.4 s | **57 / 319** (stutters) |
| zoom, `zoompan` | 2.7 s | 2.2 s | **0 / 319** |

\* frames with no change from the previous one (signalstats YDIF), a sign of stepping.
Zoom costs about +35 % render time. `zoompan` was not slow here because the input is only
~1.1× the frame (the old "very slow" note was about large inputs). On the 8 GB / integrated-GPU
target expect roughly the same ratio. **Decision: `zoompan` for photo zoom**, input pre-scaled
to frame × max zoom. Sample clips in `spike-zoom/` to watch.

### 6.3 Stock APIs (Pexels: waiting for the owner's key)
- **Openverse** (anonymous, live): `license=cc0,pdm,by` + `aspect_ratio=wide` works. Limits from
  the headers: **20 / min, 200 / day**. A 100-scene video with re-searches can reach 200, so
  the 24 h cache matters, the pause shows "Openverse daily limit — continues tomorrow", and
  Settings offers optional Openverse client credentials (the user registers their own app;
  higher "standard" tier). Results for "data center server rack": 19, **mostly Flickr at
  480–1024 px** (Openverse indexes Flickr's medium size), 3 Wikimedia files (600–819 px, real
  originals, licenses confirmed by Commons), 3 rawpixel CC0 at 2048–3000 px.
- **Wikimedia Commons search (direct)**: same query, 20 results, **13 at ≥ 1920 px wide**
  (up to 6000×4004), with license, `AttributionRequired`, `Restrictions` in one request
  (`generator=search`, `gsrnamespace=6`, `filetype:bitmap`, `iiprop=url|size|extmetadata`,
  `iiurlwidth=320` for thumbnails). **Decision: search Commons directly** (better results and
  sizes, no 200/day cap) and use Openverse for its other sources (excluding `wikimedia` to
  avoid duplicates).
- Findings: Commons contains **AI-generated images** (a DALL·E file was in the results), so
  they get the ranking penalty and are hidden (§4a). With BY-SA off, 9 of the 20 Commons results
  remain (CC0, PD, CC BY). License names vary ("CC BY-SA 2.0 fr"), so they are parsed into
  code + version + port, and unknown values are skipped (not allowed). One file had an empty
  `Artist`; the credit falls back to the file title + "Wikimedia Commons".
- **Pexels:** not tested (no key; new keys paused). The client is built to the docs and tested
  with recorded responses; live test when the owner has a key.
