# 11 — Real-world Tech Explainer (stock photos, videos, diagrams, code)

Owner request 2026-10-17. Plan, decisions and spike results:
`docs/tasks/real-world-tech-explainer-plan.md`. CLAUDE.md **exception 5** allows exactly four
services for this feature: **Pexels, Pixabay, Openverse, Wikimedia Commons** (no Unsplash,
never scraping).

Per scene the visual is one of `ai_image` (the image providers, fallback), `real_photo`,
`stock_video`, `diagram`, `code`, `screenshot`. Subtitles and the render pipeline stay as they
are.

Status: **steps 1–6 built** (data model, Settings, clients, cache, licence filter, ranking,
downloads; style preset + script fields; "Find photos & clips" job, picker, credits; render
of photos, clips, screenshots; diagrams; code / terminal scenes). Owner test next.

## 1. Data model

Scene (`pipeline/project.rs`), `visual` (older projects load as `ai_image`):

```jsonc
"visual": {
  "kind": "real_photo",            // ai_image | real_photo | stock_video | diagram | code | screenshot
  "keywords": ["submarine cable", "cable ship"],   // ENGLISH, also for Khmer scripts
  "diagram": null,                 // { "template": "flow|sequence|timeline|architecture|comparison", "mermaid": "…" }
  "code": null,                    // { "language": "bash", "text": "…", "terminal": true, "type_out": true }
  "candidates": [ /* last search, Candidate below */ ],
  "asset": "wikimedia_123456",     // chosen AssetRecord id
  "focus": [0.5, 0.4],             // subject centre for the 16:9 crop, null = centre
  "auto_picked": false,            // chosen by ranking/Gemini, shown as "check" until confirmed
  "status": "done", "error": null, "error_kind": null,
  "render_path": null,             // drawn still (diagram / code): scenes/<id>/visual.png
  "render_hash": null,             // what render_path was drawn from (stale when it differs)
  "thumb_path": "scenes/s004/visual_thumb.jpg"   // 384×216 crop around focus, grid thumbnail
}
```

`Candidate` (`stock/mod.rs`): `source` (pexels | pixabay | openverse | wikimedia | user),
`source_id`, `media` (photo | video), `title`, `tags`, `author`, `author_url`, `license`
{`code`, `version`, `url`}, `page_url` (landing page), `file_url`, `thumb_url`, `width`,
`height` (of `file_url`), `duration_ms`, `restrictions` (Commons: trademarked, personality…),
`ai_generated`, `score`, `low_res`.

Per project, `projects/<id>/assets/assets.json` + the files (`assets/<id>.jpg|png|mp4`),
`AssetRecord` (`stock/assets.rs`): `id`, `source`, `media`, `file`, `title`, `author`,
`author_url`, `license`, `original_url`, `file_url`, `downloaded_at`, `width`, `height`,
`duration_ms`, `attribution` (ready credit line), `restrictions`, `sha256`, `bytes`, `poster`
(videos: `assets/<id>.poster.jpg`, the source's preview picture). The user's own screenshots are
`user_<sha12>.jpg|png`, licence `own`, left out of the credits.
A file is downloaded once (checked by its first bytes: JPEG / PNG / MP4; photos ≤ 40 MB,
videos ≤ 200 MB) and reused by every scene and re-render.

Settings (`settings.json`): `"stock": { "pexels": true, "pixabay": true, "openverse": true,
"commons": true, "allow_by_sa": false, "auto_pick": false }`. Keys in the Windows Credential
Manager (service `NiteanStudio`): `pexels_api_key`, `pixabay_api_key`. Pexels and Pixabay are
used only when their key is stored; Commons and Openverse need none.

## 2. Sources (checked 2026-10-17)

| Source | Endpoint | Auth | Limit → what we do |
|---|---|---|---|
| Wikimedia Commons | `commons.wikimedia.org/w/api.php?action=query&generator=search&gsrnamespace=6&gsrsearch=<q> filetype:bitmap&prop=imageinfo&iiprop=url\|size\|mime\|extmetadata&iiurlwidth=320&formatversion=2&maxlag=5` | none; **User-Agent `NiteanStudio/<app version> (https://github.com/Narak76543/niteam-studio; <contact>)`** (Wikimedia policy; contact in `stock::CONTACT`) | no fixed number; one request at a time; 429 / `maxlag` → wait `Retry-After` (30 s default) |
| Openverse | `api.openverse.org/v1/images/?q=&license=cc0,pdm,by[,by-sa]&excluded_source=wikimedia&mature=false&page_size=20` | anonymous | **20 / min, 200 / day** (headers `x-ratelimit-*-anon_burst`, `…-anon_sustained`); thumbnails separately 1000 / day. 429 → `Retry-After`, or a day when `available-anon_sustained` is 0 |
| Pixabay | `pixabay.com/api/?q=&image_type=photo&orientation=horizontal&safesearch=true` and `/api/videos/?video_type=film` | `key=` query parameter (added when sending, never stored in URLs or logs) | 100 / 60 s; 429 → wait `X-RateLimit-Reset` seconds |
| Pexels | `api.pexels.com/v1/search`, `/v1/videos/search` (`orientation=landscape`) | `Authorization: <key>` | 200 / hour, 20 000 / month; no rate headers on 429 → wait 1 h |

- Wikimedia files are searched on Commons directly (more and bigger results than through
  Openverse, licence and restrictions in the same reply); Openverse covers its other sources
  (Flickr, museums, rawpixel…) with `excluded_source=wikimedia`.
- Commons: only `image/jpeg` / `image/png`, ≤ 40 MB. Author = `Artist` with HTML removed;
  title = `ObjectName` or the file name.
- Photos: Pixabay gives `largeImageURL` (≤ 1280 px) unless the account has **full API
  access** (`imageURL` original, `fullHDURL`). Videos: the largest rendition ≤ 1920 px wide
  (never 4K).
- **Pixabay full resolution:** each user applies with their own account: log in at
  pixabay.com, open pixabay.com/api/docs/ and use **"Request full API access"** (Support
  section), or write via pixabay.com/service/contact/. Pixabay asks that the API is
  "implemented properly" (we cache searches 24 h, download files, show the source); no
  published criteria or waiting time. The app picks the bigger files up automatically.
- **Openverse with registered credentials (optional, not required):** an app can register at
  `POST https://api.openverse.org/v1/auth_tokens/register/` (`name`, `description`, `email`) →
  `client_id` + `client_secret` (shown once; verify the email), then get a token at
  `POST /v1/auth_tokens/token/` (`grant_type=client_credentials`, form-encoded, valid
  36 000 s) and send `Authorization: Bearer <token>`. Authenticated clients get the
  "standard" rate tier (higher than anonymous; the docs give no numbers, the response headers
  do). Not built: the 24 h cache keeps normal use under 200 searches a day. If needed later,
  the user registers their own app (credentials are not shared or shipped).

## 3. Licensing

- Accepted: Pexels License, Pixabay Content License, **CC0**, **public domain** (PD templates,
  Public Domain Mark), **CC BY** (any version / port). **CC BY-SA only when Settings → "Allow
  CC BY-SA files" is on** (off by default: a video that crops or pans the photo may count as
  an adaptation that has to be shared under BY-SA too). Never NC or ND; unknown licence names
  (GFDL-only, "Attribution", …) are skipped (`stock/license.rs`).
- Openverse is asked for the allowed licences only; Commons `LicenseShortName` is parsed
  ("CC BY-SA 2.0 fr" → BY-SA, version "2.0 fr").
- Commons `Restrictions` (trademarked, personality rights, …) are kept and shown as a warning
  badge with the reason on hover.
- Likely AI-generated files (Commons category / title / tags with "AI-generated", "DALL·E",
  "Midjourney", "Stable Diffusion", "generative AI") are dropped.
- Credit line per asset (`license::credit_line`), stored in `assets.json`:
  - `"<title>" by <author> via Wikimedia Commons — CC BY 2.0 (<licence url>) — <page>`
  - `"<title>" by <author> — CC0 — <page>` (Openverse)
  - `Photo by <author> on Pexels — <page>`, `Video by <author> from Pixabay — <page>`
  Pexels and Pixabay do not require credit; we add it anyway (Pexels asks for it when
  possible, and it helps if a video is wrongly claimed).
- Pexels / Pixabay licence limits shown in the picker: no identifiable people in a bad light,
  no implied endorsement, no trademarks/logos for commercial goods.

## 4. Search, ranking, cache

- `stock::search::Searcher`: for each enabled source (video: Pixabay and Pexels only) →
  24 h cache (`%LOCALAPPDATA%\NiteanStudio\cache\stock\<source>\<sha256>.json`, expired files
  deleted once per app run) → one request → parse → licence filter. A failing source becomes a
  notice (`stock_no_key`, `stock_bad_key`, `stock_rate_limited` + `retry_at_utc`,
  `stock_unavailable`, `stock_bad_response`); a rate-limited source is skipped until its reset,
  the others still answer. No key or account rotation.
- **Local ranking** (`stock/rank.rs`, no AI), sum of: keyword match 0–40 (share of keyword
  words in title/tags), resolution 0 when low-res else 10–30 (30 from 2× the frame width),
  shape (landscape 10, square 4, portrait 0), licence (CC0/PD/Pexels/Pixabay 10, CC BY 8,
  CC BY-SA 5). At most half of the grid from one source while others have results.
- **Low-res** = narrower than the project frame (1920 / 1280) after the 16:9 crop
  (`min(w, h·16/9)`). **Max zoom** = `min(1.08, cropped width / frame width)`, 1.0 (no zoom)
  for low-res images.
- 6 candidates per search in the picker.

## 5. Commands

| Command | Purpose |
|---|---|
| `stock_key_set / _exists / _clear(source)` | `pexels` / `pixabay` key in the keyring |
| `stock_key_test(source)` | one live photo search; `{ok, results, error_kind, details}` |
| `stock_search(project_id?, query, keywords, media, page)` | `{candidates, notices}`; frame width from the project resolution |
| `stock_search(…, sources?)` | optional source filter (picker) |
| `stock_pick(project_id, scene_id, candidate)` | download once (+ `assets.json` record), thumbnail, set the scene's visual, confirm |
| `scene_visual_update(project_id, scene_id, edit)` | type, keywords, diagram, code, subject point (`focus`, re-crops the thumbnail), `confirm` |
| `scene_screenshot_import(project_id, scene_id, path, consent)` | own JPEG/PNG (consent required) |
| `scene_visual_preview(project_id, scene_id)` | chosen photo / poster ≤ 1280 px for the subject point |
| `project_credits(project_id)` | Credits block or null |
| `visuals_start(project_id, scene_id?) / visuals_cancel / visuals_running` | "Find photos & clips" job; events `scene-progress` (`asset: "visual"`), `generate-progress` (`job: "visual"`, `paused_quota` + `resets_at_utc`) |
| `scene_image_thumb` | real-world scenes return the visual's thumbnail unless they fell back to an AI image |

## 6. Style preset and script

- Preset **`tech_explainer`** ("Real-world Tech Explainer", `presets::is_real_world`). Its
  `style_prompt` is only used for scenes that fall back to an AI image.
- With this preset the chapter schema (`gemini/script.rs`) adds per scene: `visual_type`
  (`real_photo | stock_video | diagram | code | ai_image`), `keywords` (≤ 4 English search
  phrases), `mermaid`, `code_language` (bash, powershell, python, javascript, typescript, json,
  yaml, rust, go, java, c, cpp, sql, html, css, text), `code`, `terminal`. `image_prompt` stays
  required (fallback).
- Prompt rules (`script::visual_rules`): mostly real photos; stock video only when motion helps
  (≤ 1 in 5); about one diagram per chapter for processes / paths / architectures / timelines;
  code at most twice, only for something a viewer could type; AI image only when no photo could
  exist. Keywords: concrete, visual, most specific first, no names or years. Mermaid:
  `flowchart LR|TD`, `sequenceDiagram` or `timeline`, ≤ 8 nodes, ASCII ids, quoted labels ≤ 5
  words in the narration language (Khmer with Latin product names), no styling / HTML. Code:
  ≤ 12 lines, `$ ` prompts for terminals, documentation addresses only (example.com,
  192.0.2.x, 203.0.113.x).
- The structured-output schema keeps the added visual fields to basic string / array / boolean
  types to avoid Gemini rejecting a complex chapter schema. Allowed values are described in the
  prompt and normalized in Rust: photo/video need keywords; a diagram's Mermaid must start with
  one of the allowed headers (`script::mermaid_template`); code must not be empty; unknown code
  languages become `text`; keywords are limited to four.
- `script_job::visual_from_reply` → `scene.visual`: keywords trimmed, de-duplicated, ≤ 4;
  a diagram that still has unusable Mermaid becomes a photo (or an AI image without keywords);
  unknown code languages become `text`; `type_out` on by default.

## 7. "Find photos & clips" job (`pipeline/visual_job.rs`)

- Scenes: `real_photo` / `stock_video` without a downloaded file (or the one scene asked for).
  One at a time; saved after every scene (resumable).
- Search: the first keyword, then the second; video scenes with no clip try photos (a photo is
  better than an AI image). Ranking as in §4; the best **non-low-res** candidate is taken
  (low-res only when nothing else), up to 3 download tries.
- The pick is **downloaded right away** (so the video can be rendered) and marked
  `auto_picked` ("Check" badge, "N pictures were chosen automatically" notice) until the user
  presses **Looks good**, sets the subject point's scene, or picks another. *(Plan change: the
  plan said "pre-selected, downloaded on confirm"; a 100-scene video would then need 100
  clicks before it can render.)*
- Nothing found → `visual.status = failed`, `error_kind = stock_no_results` → the scene uses an
  AI image (`Scene::needs_ai_image`, image job + estimate only count those scenes). Real-world
  projects never share one image between two scenes.
- Every source rate-limited → `generate-progress` `paused_quota` with the earliest reset; the
  scene stays pending; **Resume** continues. No key or account rotation.

## 8. Scenes page and picker (`VisualDialog.tsx`)

- Real-world projects: header button **Find photos & clips** (instead of "Improve image
  prompts" / image prompts), progress strip, paused notice with Resume, "to check" notice.
  Cards: thumbnail of the photo / clip poster (cropped around the subject point), badges for
  type, **Check**, **Low-res**, ⚠ restrictions (reason on hover); "Choose picture" button.
  Readiness for the Render step: voice + `visualReady` (file / drawn still / AI image).
- Picker: type switch (Photo, Video clip, Diagram, Code, My screenshot, AI image); for photo /
  video a search box (English) with the scene's suggested keywords as chips, a source filter,
  6 candidates per page (thumbnail from the source, pixel size, licence, source + author,
  Low-res, ⚠ restrictions, clip length, "Source page" link), **More results**; clicking a
  candidate downloads it and uses it. Licence note for Pexels / Pixabay; "Photos provided by
  Pexels" link when Pexels results are shown (Pexels API rule).
- Subject point: the chosen photo (or poster) with the 16:9 crop box and the subtitle band
  shaded; a click moves the point (saved, thumbnail re-cropped).
- Diagram: Mermaid text; code: language, text, Terminal, Type out; screenshot: consent checkbox
  + file; AI image: uses the image prompt.
- Render page side panel: **Credits** block with **Copy** (`project_credits`).

## 9. Render (`pipeline/visual_render.rs`, `render/clip.rs`)

Per scene of a real-world project (`visual_render::plan`); AI-image scenes (and fallbacks)
keep the pan over `image_1080.png` and their old clip hash.

| Visual | Clip input | Motion |
|---|---|---|
| Photo / own screenshot | `scenes/<id>/photo_<res>_<hash>.jpg`: the 16:9 crop around the subject point, resized (Lanczos3) to **frame × zoom**, JPEG q92, made once before the clip (older ones deleted) | `zoompan` from 1 → zoom (even scenes) or zoom → 1 (odd), centred. **zoom = min(1.08, crop width / frame width)**: the last frame is 1:1, a photo is never enlarged past its own pixels; **low-res photos get no zoom** (`still`, upscaled once to the frame) |
| Stock clip | `assets/<id>.mp4` with `-stream_loop -1 -t <scene>` (a short clip loops; hard cut) | `scale` to cover the frame (`force_original_aspect_ratio=increase`), `crop` around the subject point, 30 fps; the clip's own sound is dropped |
| Diagram / code | the drawn picture (`render_path`, frame size) | still |

- Readiness: a real-world scene blocks the render when its file / drawn picture is missing
  ("N image(s) are missing"); a scene whose search failed needs its AI image instead.
- Clip hash adds the visual (asset sha, prepared file, motion, zoom / clip focus / drawn hash);
  changing the subject point or the pick re-renders only that scene.
- `zoompan` exception to the "no zoompan" rule: see 03-pipeline.md 6a (benchmark + condition).
- Editor preview: `render_timeline` gives `pan` = `zoom_in | zoom_out | still` and `zoom`;
  `scene_image_full` returns the prepared photo (made on demand) / the clip's poster cropped
  for the frame / the drawn picture. Clips preview as their poster (still).
- Checked with the real FFmpeg (`clip::tests::real_ffmpeg_renders_every_input_kind`,
  ignored by default): zoom, still and a 1 s clip looped to a 2.95 s scene, all 1920×1080.

## 10. Diagrams (`src/lib/diagramRender.ts`, `pipeline/drawn.rs`)

- Drawn in the WebView like the subtitles: `visuals_draw_plan` lists diagram / code scenes whose
  picture is missing or outdated (hash of the text + resolution + `DRAW_VERSION`; a failed
  drawing is not retried until its text changes); the UI draws a frame-size PNG and sends it
  with `visual_draw_save` → `scenes/<id>/visual.png` + `visual_thumb.jpg`; a failure is stored
  with `visual_draw_failed` (`error_kind: "diagram"`). Drawing runs automatically on the Scenes
  page whenever a diagram / code scene is pending, and again before export.
- Mermaid 11.17.2 (MIT), loaded only when a diagram is drawn (`import("mermaid")`, own chunk).
  `htmlLabels: false` (plain SVG text — an SVG with `<foreignObject>` can't be copied to a
  canvas), `securityLevel: "strict"`, theme `base` with the video palette (warm background
  `#F7F4EF`, white nodes, coral borders, timeline sections `cScale0…7` in soft tints).
- Fonts: both Kantumruy Pro files (Khmer + Latin, variable weight 100–700) are embedded in the
  SVG as base64 `@font-face` so the rasterised picture shapes Khmer exactly like the app —
  including **bold** (timeline titles); Mermaid measures labels with the same font after
  `document.fonts.load` of both weights.
- Layout: the diagram is scaled into the area above the subtitle band with a 6 % margin, at
  most ×2.6 (small diagrams stay readable, not huge).
- Picker: live preview (redrawn 0.5 s after typing stops); a diagram that doesn't parse shows
  Mermaid's error and **Fix with Gemini** (`diagram_fix`: one text request with the error,
  reply must start with an allowed header; the scene is then redrawn).
- Checked by bundling `drawDiagram` with Vite and running it in Edge (WebView2's engine):
  Khmer flowchart, Khmer timeline with bold title and section colours, sequence diagram all
  correct; broken Mermaid rejected.

## 11. Code and terminal scenes (`src/lib/codeRender.ts`)

- A dark card (editor: three dots, language name, line numbers; terminal: "Terminal", green
  prompt `$ ` / `PS> ` / `> `, commands coloured, output lines muted) centred above the
  subtitle band on the video background. At most 18 lines (longer code ends with "…"); the
  font size (18–38 px at 1080p) fits the longest line and all lines.
- Colours: Shiki 4.5.0 with the JavaScript regex engine (no WASM), theme GitHub Dark, only the
  languages in `CODE_LANGUAGES` (`text` = no colours), each loaded on first use. Terminals
  colour only the commands, with the shell grammar (bash, or PowerShell).
- Font: JetBrains Mono (variable, bundled via `@fontsource-variable/jetbrains-mono`), falling
  back to Kantumruy Pro for Khmer comments and strings.
- **Typed out** (`type_out`, default on for script-made code): besides the finished card, one
  picture per line (lines 1…k, block cursor after the last). `visual_draw_save` stores them as
  `scenes/<id>/code/000.png…` (`visual.frames`); the render job writes
  `scenes/<id>/code/track.ffconcat` with the voice timing (`drawn::code_track`: the lines
  appear over the first 60 % of the voice, at most 0.9 s per line; the full card stays until
  the clip ends) and the clip uses it as an `Input::Frames` background. One-line code and
  `type_out: false` are a still.
- Picker: live preview of the card. The editor preview shows the finished card.
- Checked by bundling `drawCode` with Vite and running it in Edge: a terminal traceroute card
  (6 frames) and a Python editor card with a Khmer comment; real FFmpeg renders a frame track.

