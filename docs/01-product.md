# 01 — Product

## What it is

A desktop tool that turns a **topic** ("Where did rice come from?") into a finished
**MP4 explainer video** of 1–20 minutes, narrated in Khmer or English, in the style of
faceless YouTube "why / history / science" channels.

## Who it is for

Cambodian content creators running faceless educational channels on ordinary
Windows laptops. They are not engineers. They have (or can create) a Gemini API key.

## The video format

- Narration: one voice, storytelling tone, hook in the first 10 seconds
- Visuals: one AI illustration per scene (6–8 s), consistent art style across the
  whole video, gentle pan movement so stills feel alive
- Subtitles: burned in, 1–2 short lines, readable on phones
- Background music: low volume, ducked under narration
- Output: 16:9 MP4, 1080p (720p option), plus a thumbnail image

## User flow

1. **First run** → Settings: paste Gemini API key, test it, choose output folder.
2. **New project**: topic, language (Khmer / English), target length (1–20 min),
   style preset (e.g. "storybook watercolor", "cinematic realistic", "flat 2D"),
   voice choice.
3. **Script**: app generates a scene-by-scene script. User edits narration and image
   prompts, can add/delete/reorder scenes, then clicks **Approve**.
   Nothing expensive is generated before approval.
4. **Generate**: voice + image for every scene, in parallel, with progress per scene.
5. **Review**: scene grid — see image, play audio, regenerate one scene's image or
   voice, edit text and regenerate.
6. **Render**: choose resolution and music, render with progress + ETA + cancel.
7. **Done**: open MP4, open folder, thumbnail shown.

Projects are saved; the user can close the app and continue later from any step.

## Non-goals (for now)

- No local AI models, no offline mode
- No upload to YouTube from the app
- No multi-speaker dialogue, no avatar/face video
- No macOS/Linux builds (code should not block them, but we don't test them)
- No image-to-video animation (maybe later as an option)

## Quality bar

- The Khmer must read and sound natural. Wrong word breaks in subtitles or
  mispronounced numbers count as bugs.
- A 20-minute video must render on an 8 GB RAM laptop without crashing.
- A crash or closed app must never lose finished work.
