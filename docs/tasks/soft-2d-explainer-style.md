# Task: "Soft 2D Explainer" style preset (Local Z-Image)

Owner brief, 2026-10-17. Status and results: see "Results" at the bottom.

## Goal
Add a new image style preset, **Soft 2D Explainer**, next to Stickman, Storybook watercolor,
Cinematic realistic and Flat 2D. Make it work with the **Local GPU (Z-Image)** provider, so the
next video can be generated in this style.

Target look (reference: a Khmer history explainer, a style we like):
- Flat 2D cartoon, calm explainer-video feel.
- **Characters:** simple, round bald heads (peach skin tone), dot eyes, small smiles, thin stick
  legs, clean **thick black outlines**. Clothing changes per era (fur cloaks, robes, uniforms…).
- **Background:** soft painterly style with **no outlines**: layered rolling hills fading into
  mist, soft overcast sky with clouds.
- **Palette:** muted greens, sage, grey-blue, soft earth tones. Gentle light. Lots of empty space.
- **No text, letters, logos, watermarks or badges** in the image. Subtitles and any mascot or
  channel badge are overlays in Nitean Studio.

⚠️ Build our OWN characters and look. Don't copy any specific channel's mascot or character design.

## Step 0: Check before coding (report back, then stop)
1. Is a Z-Image model profile listed in `local_models.json`? If not, add it (files, sha256,
   sizes, recommended steps/CFG/sampler). Confirm "Local GPU" can be selected in the app.
2. Generate ONE test image with the prompt below at 1280×720 on the RTX 4050 (6 GB). Report time
   per image, peak VRAM, and whether the OOM retry kicked in.
3. Check whether this model uses a negative prompt with our settings. If it ignores it
   (e.g. CFG = 1), don't rely on negatives. Put the rules in the positive prompt.

Test prompt:
> Flat 2D cartoon illustration, simple explainer-video style. Small group of prehistoric people
> with round bald peach-coloured heads, dot eyes, small smiles, wearing shaggy brown fur cloaks,
> thin stick legs, clean thick black outlines on the characters. Standing together on rolling
> green hills. Soft painterly background with no outlines: layered misty hills fading into the
> distance, overcast grey-blue sky with soft clouds. Muted green, sage and grey palette, gentle
> lighting, lots of empty space, no text, 16:9.

## Step 1: Add the preset
- Add `Soft 2D Explainer` to the style presets, same structure as the existing ones.
- **Style text:** the test prompt's style parts (everything except the scene content).
- **Character rule** (like the stick-figure rule): the Visual Bible and scene prompts describe
  every person with the SAME base design (round bald peach head, dot eyes, stick legs, black
  outline) and vary them only by clothing colour/type, one prop, or hair accessory. Places are
  soft painted landscapes or simple buildings, without outlines.
- Visual Bible: written by the Gemini text model using the rule above; editable in Look & feel.
- Always add "no text, no letters, no watermark" to the final prompt for this preset.
- Don't change the Stickman default or other presets.

## Step 2: Test project
About 1 minute, Khmer history, 6 scenes:
1. Prehistoric people gathered on green hills (the reference scene).
2. A village of stilt houses by a river.
3. Farmers planting rice in flooded fields.
4. A king on a simple throne with two advisers.
5. Workers building a stone temple.
6. A crowd celebrating with lanterns at night.

3 seeds per scene. Report time per image and total, character consistency, outline/background
split, any text or garbage letters, best seeds. Save a contact sheet. (Owner, 2026-10-17: also
one scene at 1280×720 next to its 1024×576 version, and how 1024×576 looks scaled to the 1080p
export.)

## Step 3: Fixes based on results
- Characters drift → strengthen the Bible rule, repeat the same character description word for
  word in every scene prompt.
- Outlines on the background → reword ("painterly background, soft brush shading, no line art").
- Style weak → note as a candidate for a small style LoRA later (20–40 approved images). Don't
  train now.

## Out of scope
Mascot/badge overlay, training a style LoRA, changing the Cloudflare/Gemini providers (the preset
must still work with them; test on Local first).

## Done when
- [x] Z-Image profile listed and selectable as Local GPU
- [x] Soft 2D Explainer preset added, with its character rule
- [x] 6-scene test project generated, contact sheet saved
- [x] Report: timings, VRAM, consistency notes, recommended seeds/settings
- [x] Docs updated (style presets list, local models doc)

## Results (2026-10-17)

Test folders: `D:\development\nitean-studio-tests\soft-2d-explainer\round1\` and `round2\`
(`contact_sheet.png`, single images, `report.md` / `run.log`). Harness:
`src-tauri/src/pipeline/style_trial.rs` (ignored test). Both projects are in the app's project
list ("ប្រវត្តិសាស្ត្រខ្មែរ ក្នុងមួយនាទី", ids `01M53QZG…` round 1, `01M53RNW…` round 2).

- Speed (RTX 4050 6 GB): 1024×576 28.4 s / image warm, 44 s first (load); 18 images 527 s.
  1280×720 54–56 s. Peak VRAM 4.4–4.7 GB. No OOM retry.
- Characters: same round bald head, dot eyes, smile, black outline in all 34 images; clothing
  varies as the Bible says (fur/tunic, gold robe + crown, farmer).
- Text: none (one tiny corner mark in round 1, scene 4, seed 93688).
- Round 1 problem: Bible `culture_notes` listed every Cambodian object → temples and rice
  paddies in every scene (also prehistoric), night scene in daylight; background buildings
  outlined. Fixed: culture notes rule (one phrase that fits every scene), style reworded
  ("no line art", "few background elements", "only the people have outlines").
- Round 2: every scene on its own subject, calmer backgrounds, buildings still lightly outlined.
  Night scene still dusk. Round 2 was stopped by the host (low system memory) after 16 of 18
  images (scene 6: 1 seed).
- Best seeds, round 2: s1 2800, s2 2802, s3 2802, s4 2803, s5 2805, s6 2804.
- LoRA candidate: not needed for the figures; possibly later for the misty, outline-free
  backgrounds of the reference look.
