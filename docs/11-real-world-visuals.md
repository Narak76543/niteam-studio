# 11 — Real-world Tech Explainer (stock photos, videos, diagrams, code)

Owner request 2026-10-17. Plan, decisions and spike results:
`docs/tasks/real-world-tech-explainer-plan.md`. CLAUDE.md **exception 5** allows exactly four
services for this feature: **Pexels, Pixabay, Openverse, Wikimedia Commons** (no Unsplash,
never scraping).

Per scene the visual is one of `ai_image` (the image providers, fallback), `real_photo`,
`stock_video`, `diagram`, `code`, `screenshot`. Subtitles and the render pipeline stay as they
are.

Status: **step 1 built** (data model, Settings, clients, cache, license filter, ranking,
downloads). Script preset, picker UI, render, diagrams, code: next steps.

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
  "render_path": null              // final still: scenes/<id>/visual.png|jpg
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
`duration_ms`, `attribution` (ready credit line), `restrictions`, `sha256`, `bytes`.
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
| `stock_pick(project_id, scene_id, candidate)` | download once (+ `assets.json` record), set the scene's visual |
