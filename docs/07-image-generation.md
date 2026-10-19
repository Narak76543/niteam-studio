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
`storybook_watercolor`, `cinematic_realistic`, `flat_2d`, `soft_2d_explainer` (2026-10-17, see
below). Stickman prompt: "simple hand-drawn
stick figure doodle, bold clean black marker lines, flat pure white background filling the
whole frame, stick figures with round heads and simple dot eyes, complete scene drawn in the same
doodle lines with a background setting and several topic props, flat colour accents, whiteboard
explainer style" (2026-10-16: "lots of white space" / "minimal" removed — frames looked empty;
owner asked for fuller, on-topic scenes). Existing projects keep their stored style text until
the style is picked again in Look & feel. (Do not say "on white paper": Flux then draws a
photo of a sheet of paper on a wall.) Checked with Flux Schnell on 2026-10-15: clean doodles,
props (cap, scarf, hat) stay recognisable. Look & feel → **Style** switches an existing project
(`project_set_style`: new preset + Visual Bible rewritten for it; images become outdated).

**Soft 2D Explainer** (`soft_2d_explainer`, owner 2026-10-17, brief in
`docs/tasks/soft-2d-explainer-style.md`): calm explainer cartoon, our own figure design.
- Style: polished flat 2D educational illustration with one recurring stickman guide as the main
  character: round peach head, dot eyes, small smile, thin black stick body and limbs, and a small
  teal scarf. Keep the guide prominent and interacting with the scene's exact subject. Use crisp
  silhouettes, balanced composition, purposeful soft-painted details, restrained texture, gentle
  light, muted green/sage/grey-blue and earth tones, and intentional empty space.
- **Preset rules** (`StylePreset::prompt_rules`, added word for word to every final prompt): clearly
  show the exact scene object, event or action; show changes for process scenes; avoid generic people,
  crowds and landscapes. For AI, software or coding scenes, include the specific computing tools and
  developer action from the narration. Historical and prehistoric scenes use period-correct details and exclude modern objects unless the scene explicitly compares eras. Vary setting and composition between scenes; keep the recurring
  stickman guide interacting with the subject. Never draw writing, pseudo-text, numbers, signs, labels,
  titles, logos, badges or watermarks. Video topic wording is omitted to reduce accidental image text.
  Negative phrasing is deliberate: Z-Image runs at cfg 1.0, where the negative prompt does
  nothing, and its Qwen3 encoder follows "no …" in the positive prompt (08 §13).
- Figures rule: see §2 (`Figures::SharedCartoon`).
- Tested 2026-10-17 on local Z-Image (two 6-scene Khmer history rounds, 3 seeds each,
  `pipeline/style_trial.rs`): same head, eyes and outline in every image, no text. Round 1 had
  outlined, busy backgrounds ("soft sky with gentle clouds", "background painted softly") — reworded
  to "no line art", "few background elements", "only the people have outlines": calmer, softer
  backgrounds, buildings keep a thin outline. Night scenes still come out as dusk.

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

The style text decides how people are described (`presets::figures`, read from the text so an
edited style keeps its rule):
- **Stick figures** ("stick figure" / "stickman"): the bible prompt adds that characters are
  stick figures described ONLY by one or two bold props / colour accents ("stick figure with a
  red-and-white checked krama scarf and a straw hat"), never skin, faces, hair or body detail;
  places are simple line drawings with at most two flat colour accents.
- **Shared cartoon** ("round bald", the Soft 2D Explainer preset): every person has the same
  base design (added by the preset rules, §1c); characters are described ONLY by clothing type
  and colour, at most one prop and one headwear / hair accessory ("figure in a long red robe with
  a small gold crown"); places are soft painted landscapes or simple buildings in the preset's
  palette; the bible's `style` is the chosen style copied word for word. Scene `image_prompt`s
  get the same rule (clothing, prop, action; never faces or bodies).

`culture_notes` may be added to an image (§3), so the bible prompt asks for one short phrase
(≤ 25 words) that fits every scene — clothing style, roof / carving shapes, landscape — and never a
list of objects or events from only some scenes (2026-10-17: a list of "stilt houses, rice
paddies, temples, crowns, lanterns" put an Angkor temple into the prehistoric scene of every seed).
For Soft 2D, these notes are included only when the project title or topic itself mentions Cambodia,
Khmer, or a related place in English or Khmer; Khmer narration alone does not make an unrelated
topic Cambodian. The final image prompt uses the scene's specific visual description instead of
copying the project's title or raw topic wording into the prompt.

Rules for the script model:
- Every recurring person/place gets **one** bible entry; scenes refer to it by key.
- Vary `shot` so no more than 2 consecutive scenes share the same shot type.
- Mark scene 1, each chapter's first scene and the last scene as `key_scene: true`.
- **`image_prompt` explains the narration** (`script::image_prompt_rules`, 2026-10-16): 40–80
  words; the exact object/event/number the scene talks about; main subject + key action,
  2–4 topic-specific details (no generic "artifacts"), a setting that changes between scenes;
  abstract ideas become visible devices (side-by-side, before/after, left-to-right sequence,
  cutaway, magnified inset, size comparison, map shape). No style/lighting/camera words; sensitive
  topics stay tasteful. Stick-figure styles: people shown by props and actions only;
  shared-cartoon styles: by clothing, one prop and actions only.

## 3. Prompt building (`images/prompt.rs`, pure function + unit tests)

```
{style}, {shot phrase}, {subject from image_prompt with bible descriptions inlined},
{place description}, {culture_notes if Cambodia-related}, {composition phrase},
{preset rules (§1c), never cut}, clean image without any lettering
```

- **Shot phrases:** wide → "wide establishing shot"; medium → "medium shot";
  close → "close-up portrait"; detail → "close-up detail shot"; aerial → "aerial view".
- **Composition phrase for square providers:** "wide horizontal composition, main
  subject in the center band, empty space above and below". Needed because we crop to 16:9.
- **Positive phrasing only.** Flux Schnell ignores negative prompts: write
  "clear blue sky", not "no birds".
- **Never ask for text in the image.** Titles/subtitles are drawn by the app.
- No real living people. Historical figures only as statues/carvings or generic figures.
- Truncate to 2,048 chars (Flux limit), dropping `culture_notes` and place first. If oversized style descriptions still exceed the limit, trim those before cutting the scene subject so topic-specific objects and actions remain.
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
