# 07 — Image Generation

Images are the most expensive step and decide how professional the video looks.
This doc defines **providers**, the **Visual Bible** (consistency), **prompt building**,
**post-processing**, **quota handling** and **caching**.

> Before writing provider code, read the provider's current API docs and confirm the
> request/response format. The details below were checked on 2026-10-15.

## 1. Providers

All providers implement one trait (`src-tauri/src/images/provider.rs`):

```rust
#[async_trait]
pub trait ImageProvider: Send + Sync {
    fn id(&self) -> &'static str;                 // "cloudflare_flux", "gemini_nano_banana"
    fn supports_reference_images(&self) -> bool;
    fn native_aspect(&self) -> Aspect;            // Square | Wide16x9
    async fn generate(&self, req: &ImageRequest, cancel: &CancellationToken)
        -> Result<GeneratedImage, ImageError>;
}

pub struct ImageRequest {
    pub prompt: String,          // fully built prompt (section 3)
    pub seed: u32,
    pub steps: Option<u8>,
    pub reference_images: Vec<PathBuf>, // ignored if unsupported
}

pub struct GeneratedImage { pub bytes: Vec<u8>, pub mime: String, pub width: u32, pub height: u32 }

pub enum ImageError {
    QuotaExhaustedDaily { resets_at_utc: DateTime<Utc> }, // pause the queue
    RateLimited { retry_after: Option<Duration> },        // backoff + retry
    SafetyBlocked(String),                                 // user edits prompt
    Auth(String),                                          // bad token/key, stop
    Transient(String),                                     // 5xx/timeout, retry
    Other(String),
}
```

| Provider id | Model | Tier | Notes |
|---|---|---|---|
| `cloudflare_flux` (**default**) | Cloudflare Workers AI `@cf/black-forest-labs/flux-1-schnell` | Free daily allowance | Square JPEG, text prompt only |
| `gemini_nano_banana` | `gemini-3.1-flash-lite-image` / `gemini-nano-banana-2.1` / `gemini-3-pro-image` | Paid only | 16:9 native, reference images |

Settings: `image.provider`, `image.gemini_model`, `image.steps` (4–8, default 4),
`image.key_scene_provider` (optional better provider for hook/chapter/ending scenes).

### 1a. Cloudflare Flux Schnell

- **Endpoint:** `POST https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/ai/run/@cf/black-forest-labs/flux-1-schnell`
- **Headers:** `Authorization: Bearer {API_TOKEN}`, `Content-Type: application/json`
- **Body:** `{ "prompt": string (1–2048 chars), "steps": int (max 8, default 4) }` — nothing else.
  The live API rejects extra properties: sending `seed` returns HTTP 400 "Additional or
  unevaluated properties '/seed' at '/' not allowed" (seen 2026-10-15, although the docs'
  examples show one). So Flux output is **not** reproducible from a seed.
- **No width/height parameter** → output is a **square JPEG**.
- **Response:** JSON; base64 JPEG at `result.image`. Check `success` and `errors[]`.
- **Credentials:** Account ID (stored in `settings.json`, not secret) and API token
  (keyring: service `NiteanStudio`, user `cloudflare_api_token`). Never log the token.
- **Free allowance:** 10,000 neurons/day, resets **00:00 UTC (07:00 in Cambodia)**.
  Cost ≈ 4.80 per 512×512 tile + 9.60 per step → ~58 neurons per image at 4 steps
  → **~170 images/day**. Treat these numbers as estimates; read real usage from errors.
- **Daily quota exhausted** (Cloudflare returns an error indicating the neuron limit):
  map to `QuotaExhaustedDaily { resets_at_utc: next 00:00 UTC }`.
- **"Test connection"** button in Settings: one 1-step generation of "a small green leaf".
- License: Flux Schnell terms are linked from the Cloudflare model page; show a link in Settings.

### 1b. Gemini Nano Banana (paid)

- Same Gemini API key as script/voice. **Not available on the free tier** → on a
  free-tier error, show "Gemini image models need billing. Switch to Cloudflare Flux
  in Settings."
- Request 16:9 directly. Pass up to 3 reference images (approved style/character images).

### 1c. Style presets

`pipeline/presets.rs`: **`stickman` (default since 2026-10-15, owner's choice)**,
`storybook_watercolor`, `cinematic_realistic`, `flat_2d`. Stickman prompt: "simple hand-drawn
stick figure doodle, bold clean black marker lines, flat pure white background filling the
whole frame, stick figures with round heads and simple dot eyes, minimal flat colour accents,
whiteboard explainer style, lots of white space". (Do not say "on white paper": Flux then draws a
photo of a sheet of paper on a wall.) Checked with Flux Schnell on 2026-10-15: clean doodles,
props (cap, scarf, hat) stay recognisable. Look & feel → **Style** switches an existing project
(`project_set_style`: new preset + Visual Bible rewritten for it; images become outdated).

## 2. Visual Bible (consistency)

Generated **once per project**, right after the script, by the text model. Stored in
`project.json` as `visual_bible` and editable in the Script page ("Look & feel" tab).

```json
{
  "style": "warm watercolor storybook illustration, soft golden light, muted earth palette, gentle paper texture",
  "characters": {
    "farmer": "elderly Khmer rice farmer, thin, dark tan skin, white checked krama scarf on head, faded blue shirt, black trousers rolled to the knee"
  },
  "places": {
    "village": "Cambodian village of wooden stilt houses, sugar palm trees, flooded green rice paddies"
  },
  "culture_notes": "Cambodian Khmer setting: Khmer sampot clothing, Angkorian sandstone carvings, Khmer roof shapes with upturned finials",
  "seed": 1847
}
```

Script JSON per scene gains: `"characters": ["farmer"]`, `"place": "village"`,
`"shot": "wide|medium|close|detail|aerial"`, `"key_scene": bool`.

For **stick-figure styles** (`presets::is_stick_figure`) the bible prompt adds: characters are
stick figures described ONLY by one or two bold props / colour accents ("stick figure with a
red-and-white checked krama scarf and a straw hat"), never skin, faces, hair or body detail;
places are simple line drawings with at most two flat colour accents.

Rules for the script model:
- Every recurring person/place gets **one** bible entry; scenes refer to it by key.
- Vary `shot` so no more than 2 consecutive scenes share the same shot type.
- Mark scene 1, each chapter's first scene and the last scene as `key_scene: true`.

## 3. Prompt building (`images/prompt.rs`, pure function + unit tests)

```
{style}, {shot phrase}, {subject from image_prompt with bible descriptions inlined},
{place description}, {culture_notes if Cambodia-related}, {composition phrase},
clean image without any lettering
```

- **Shot phrases:** wide → "wide establishing shot"; medium → "medium shot";
  close → "close-up portrait"; detail → "close-up detail shot"; aerial → "aerial view".
- **Composition phrase for square providers:** "wide horizontal composition, main
  subject in the center band, empty space above and below". Needed because we crop to 16:9.
- **Positive phrasing only.** Flux Schnell ignores negative prompts: write
  "clear blue sky", not "no birds".
- **Never ask for text in the image.** Titles/subtitles are drawn by the app.
- No real living people. Historical figures only as statues/carvings or generic figures.
- Truncate to 2,048 chars (Flux limit), dropping `culture_notes` first, then place.
- **Seed:** `visual_bible.seed + scene_index`. "Regenerate" in the UI uses a new random
  seed and stores it on the scene. Cloudflare Flux does not accept a seed (§1a): there the
  seed is used only in the cache hash (§8), so a new seed still forces a fresh image, but
  the same seed doesn't produce the same picture again.

## 4. Post-processing (`images/postprocess.rs`, `image` crate)

1. Save the provider's original as `scenes/<id>/image_raw.jpg` (or .png).
2. If square: **center-crop to 16:9** (e.g. 1024×1024 → 1024×576), keeping the vertical
   center (composition phrase puts the subject there).
3. Upscale with **Lanczos3** to the pan size: 2150×1210 for 1080p, 1434×806 for 720p
   → `image_1080.png` / `image_720.png`.
4. Create a 384×216 `thumb.jpg` for the UI grid (fast loading on weak laptops).

## 5. Budget & scheduling

- Project option **Scenes per image**: 1 (default) or 2. With 2, consecutive scene
  pairs share one image; the second scene uses the opposite pan direction.
- Before generating, show an estimate:
  "86 images · 80 Cloudflare (free, ~47% of today's allowance) · 6 Gemini (~$0.20)".
- Concurrency for Cloudflare: 2 at a time (default), configurable.
- Generation order: **key scenes first** (so the user sees the look early), then in order.

## 6. Quota pause & resume

- On `QuotaExhaustedDaily`: cancel pending image jobs, keep finished ones, save state,
  set project `status = "paused_quota"`, show:
  "Daily free image limit reached. N of M images done — progress saved.
  Resume after 7:00 AM." with a **Resume** button and the time computed in local time.
- On app start, if `paused_quota` and the reset time has passed, show "Ready to resume".
- Never switch to another account or token to continue.

## 7. Quality check (optional, Phase 5)

After each image, send `thumb.jpg` to a free Gemini Flash text model with vision:
"Answer JSON {has_text, distorted_people, matches_prompt (0-10)} for this prompt: …".
If `has_text` or `distorted_people` or `matches_prompt < 5` → mark `needs_review`
(and optionally auto-regenerate once with a new seed). Off by default; never blocks render.

## 8. Caching

`image.prompt_hash = sha256(provider_id + model + final_prompt + seed + steps)`.
If hash matches and the file exists → skip. Changing style, bible, prompt, provider or
seed marks the image `stale`.

## 9. Thumbnail (Phase 5)

Use `key_scene_provider` if set (best model), prompt: one dramatic close-up subject,
strong emotion or striking object, clean background on one side for the headline.
The Khmer/English title is drawn by the app with the canvas method (see `04-khmer.md`).

## 10. Tests

- `prompt.rs`: bible inlining, shot phrases, 2,048-char truncation order.
- `postprocess.rs`: square → 16:9 crop dimensions, upscale sizes.
- Cloudflare client: mocked HTTP for success, auth error, quota error, 5xx retry.