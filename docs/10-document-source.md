# 10 — Video from a document

The user picks a PDF (paper, article, report); Gemini reads it, the user picks the key points,
and the normal pipeline (outline → Visual Bible → scenes → images → voice → subtitles → render)
makes the video. Owner brief and approval: 2026-10-17 (plan approved with a 150-page cap, `lopdf`,
and "must work for FREE on the Gemini free tier").

## 1. Flow

1. **New project → "From a document"** (next to "From a topic"). Pick a PDF; the app shows name,
   pages, size. Options: video language (Khmer / English, may differ from the PDF), target length,
   audience (general / students / experts), optional focus ("explain only the results section"),
   page range (only for PDFs over 150 pages), style preset. Free-tier note on the screen:
   "Free tier: Google may use uploaded content to improve its services. Don't upload private
   documents."
2. **Read** (`source_job`, §3): section notes per chunk, then the **Source summary**.
3. **Review** (Script page, §6): tick / untick, reorder, notes. Nothing else is generated until
   **Confirm and write script**.
4. **Script** from the selected points only (§7); the rest of the pipeline is unchanged.
5. Each scene keeps its **source pages** (Script page editor) and a **general knowledge** flag.
6. **Credit line** ("Source: authors, "title" (year)") in the editor's side panel with Copy.

## 2. Data (`project.json`)

`project.source` (`pipeline/project.rs::SourceDoc`):

```jsonc
"source": {
  "file_name": "rice-report.pdf", "size_bytes": 63792, "page_count": 3, "sha256": "…",
  "page_from": 1, "page_to": 3,              // pages read (≤ 150)
  "audience": "general",                     // general | students | experts
  "focus": "",
  "stage": "review",                         // reading | review | confirmed
  "chunks": [{ "from": 1, "to": 3, "mode": "pdf",   // text | pdf | null (not read yet)
               "sections": [{ "heading": "Results", "notes": "…", "pages": [2] }],
               "title": "…", "authors": "…", "year": "2024", "done": true }],
  "summary": {
    "title": "…", "authors": "…", "year": "…",
    "key_points": [{ "id": "k1", "text": "…", "pages": [1], "selected": true, "note": "" }],
    "facts":      [{ "id": "f1", "text": "…", "pages": [2], "selected": true, "note": "" }],
    "terms":      [{ "id": "t1", "term": "SRI", "text": "…", "pages": [1, 2], "selected": true, "note": "" }]
  },
  "pause": null,                             // { "resets_at_utc": "…", "daily": true }
  "credit": "Source: …"                      // set on confirm
}
```

Scenes: `source_pages: [u32]`, `general_knowledge: bool`. The PDF is copied to
`projects/<id>/source/document.pdf`. The project stays in `script_draft` the whole time; the
Script page shows the reading / review screen while `source.stage != "confirmed"`.
Settings: `models.document` (default `["gemini-3.8-flash"]`, Settings → Models).

## 3. Reading (`pipeline/source_job.rs`, `gemini/document.rs`, `source/*`)

- The page range is cut into **50-page chunks** (`source::pdf::chunks`); one Gemini call each,
  then one **text-only** call that turns all section notes into the summary (long documents are
  summarised per section first).
- **Text first, PDF as fallback** (saves quota and tokens): `lopdf` extracts each page's text;
  `source::quality::check` accepts it only if ≥ 70 % of the pages have ≥ 80 visible characters,
  < 0.5 % NUL / U+FFFD / control / private-use characters, < 1 % Khmer signs in impossible
  positions (dependent vowel after a space, digit or punctuation; coeng without a consonant) and
  ≥ 50 % letters. Then the text is sent with `=== Page N ===` markers; otherwise the chunk's pages
  are cut into their own PDF (`extract_range`) and sent inline. The log says which mode each chunk
  used and why ("pages 1–3: sent as PDF pages (unreadable characters in the text)").
- Khmer PDFs made by browsers / Word usually fail the check (lost subscripts, pre-base vowels in
  visual order — measured 2026-10-17), so Khmer documents mostly go as PDF pages; English PDFs
  with embedded text go as text.
- **Request:** `generateContent`, user turn = `inlineData { mimeType: "application/pdf", data }`
  (PDF chunks) + instructions, JSON schema output (`script::request_json_with`, own system prompt
  `READER_SYSTEM`: faithful, every number with its unit, cite pages). Live-checked on a free key
  2026-10-17; the Interactions API `document` part works too, but `generateContent` reuses our
  retry / chain code. No Files API: chunks are small enough to send inline (limit 50 MB / 1,000
  pages per PDF), and nothing stays stored at Google for 48 h.
- Page numbers are PDF page numbers (as a PDF viewer shows them), not the printed ones; pages
  outside the chunk / range are dropped.
- **Cost:** a PDF page sent as pages is ~520 input tokens (measured: 2 pages = 1,040 image tokens);
  text pages cost their text tokens. A 20-page PDF = 1 chunk + 1 summary = 2 calls; 100 pages = 3
  calls.

## 4. Limits

| Case | What happens |
|---|---|
| > 50 MB | Blocked: "This file is larger than 50 MB. Compress it, or save only the chapters you need as a new PDF." |
| > 150 pages | The upload step asks for a page range of at most 150 pages; only that range is read. |
| Password-protected / damaged / no pages | Clear message, no project created. |

## 5. Free tier: models and quota

- Owner rule (2026-10-17): **free-tier models only.** Live test on the owner's free key: all three
  configured models accept PDFs (gemini-3.8-flash, 3.5-flash-lite, 3.1-flash-lite). On a scanned
  Khmer page only **gemini-3.8-flash** got every number and province right; the Flash-Lite models
  invented details ("Kampong Thom", "75 hectares"). So `models.document = ["gemini-3.8-flash"]`,
  and the reader does **not** fall back to Flash-Lite.
- **Script** of a "From document" project: `ModelSettings::script_chain(true)` = document models
  first, then the normal text chain (Flash-Lite turned 28.4 BLEU into "29.4" in one test).
  A daily quota on one model moves the chain to the next model (free-tier quotas are per model).
- **Quota seen on 2026-10-17:** gemini-3.8-flash free tier returned
  `limit: 20` requests per day (`quotaId: GenerateRequestsPerDayPerProjectPerModel-FreeTier`,
  `retryDelay: 76787s`, i.e. midnight UTC — not the documented midnight Pacific). Requests
  worked again a few minutes later, so Google's counting is not a simple daily counter.
- **Pause, never switch keys:** a 429 that outlasts the retries pauses the reading job
  (`source.pause`): daily quota (`PerDay` in the body) → until the server's `retryDelay`, else
  the next midnight Pacific (`source::reset`); per-minute limit → `retryDelay` or 60 s. A daily
  quota is not retried (`GeminiError::is_retryable`). The Script page shows the time and a
  **Continue reading** button; the job resumes at the first chunk that isn't `done`.

## 6. Review screen (Script page, `components/SourceReview.tsx`)

Title / authors / year, file and pages read, **Read again**. Key points as cards: checkbox, text in
the project language, page chips (click → that page's section notes), **Add note**, move up/down.
Facts and terms below as tick lists. Footer: "N of M points selected · about X min of material for
a Y-min video", "Nothing generated yet", **Confirm and write script**. Edits save after 0.6 s.
Confirm needs ≥ 1 key point; it sets the credit line and adds the selected Latin-script terms to
`keep_terms` (Khmer narration keeps them in English). After the script exists, **Back to key
points** (Script page) removes the script so it can be written again from a new selection.

## 7. Script rules (`document::source_block`, `script::outline_prompt` / `chapter_prompt`)

The outline and every chapter get the source block: selected key points in the user's order
(with notes), selected facts and terms, the section notes (≤ 12,000 characters) and:
- every claim, number and result must come from the points, facts or notes — no added facts;
- background explanation is allowed but is general knowledge: `general_knowledge: true`,
  `source_pages: []`; the title, hook and ending follow the same rule (no "every modern AI uses
  this" unless the document says so);
- `source_pages` per scene (kept only inside the read range; cleared for general knowledge);
- Khmer: technical terms stay in English where a Khmer presenter would say them in English
  (summary prompt + `keep_terms`).

## 8. Tests

- Unit: chunking and ranges, text-quality check (clean EN/KM pass; empty, NUL-garbled and
  visual-order Khmer fail), Pacific reset incl. DST, daily vs per-minute 429, `retryDelay`
  parsing, summary/review/confirm/reopen, prompts and schema with/without source, scene page
  refs, and **pause → resume at the open chunk** with a mocked Gemini.
- Live (ignored): `pipeline/source_trial.rs` (`NITEAN_PDFS`, `NITEAN_LANG`, `NITEAN_SCRIPT=1`).

## 9. Live results (2026-10-17, free key)

| PDF | Mode | Time | Result |
|---|---|---|---|
| "Attention Is All You Need" (15 p., English, arXiv) | text | 38–55 s | 8 key points, 14 facts, 8 terms, all numbers correct (28.4 / 41.8 BLEU, 8 P100, 3.5 days) |
| Khmer report (3 p., Edge-printed, garbled text layer) | PDF | 72 s | 7 points, all numbers / provinces correct |
| Same report as image-only scan (2 p.) | PDF | 23 s | 6 points, all correct |

Khmer script (1 min, 9 scenes) from the English paper: every scene has page refs; English terms
kept (Transformer, self-attention, BLEU); after the hook/ending rule the ending is marked general
knowledge. One run failed in the first chapter (cause not captured; reruns passed) and a network
error was retried once.
