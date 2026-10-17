import { Captions, ImageIcon, Maximize2, Volume2, ZoomIn, ZoomOut } from "lucide-react";
import { useEffect, useMemo, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";

import { IconButton } from "../Button";
import { SceneThumb } from "../SceneThumb";
import { useI18n } from "../../i18n/context";
import { cueText, type SubtitleState } from "../../lib/subtitleStates";
import type { Language, Scene, TimelineScene } from "../../lib/types";
import { followScroll, formatTime, sceneIndexAt } from "../../lib/timeline";

/** Clips narrower than this show no thumbnail (at the smallest zoom most clips are 28–36 px). */
const MIN_THUMB_PX = 12;

const MIN_PPS = 4;
const MAX_PPS = 200;
const LABEL_W = 44;
const ICON = { size: 16, strokeWidth: 1.5 } as const;

interface Cue {
  start_ms: number;
  end_ms: number;
  text: string;
}

interface Props {
  projectId: string;
  lang: Language;
  scenes: TimelineScene[];
  /** Project scenes (thumbnail versions). */
  projectScenes: Scene[];
  states: SubtitleState[][];
  timeMs: number;
  totalMs: number;
  onSeek: (ms: number) => void;
}

/** Cue blocks (absolute times) from the per-scene subtitle states. */
function cuesOf(scenes: TimelineScene[], states: SubtitleState[][]): Cue[] {
  const out: Cue[] = [];
  scenes.forEach((sc, i) => {
    let prev: SubtitleState["lines"] | null = null;
    for (const st of states[i] ?? []) {
      const start = sc.start_ms + st.start_ms;
      const end = Math.min(sc.start_ms + sc.clip_ms, sc.start_ms + st.end_ms);
      const last = out.at(-1);
      if (st.lines === prev && last) last.end_ms = end;
      else out.push({ start_ms: start, end_ms: end, text: cueText(st.lines).replace("\n", " ") });
      prev = st.lines;
    }
  });
  return out;
}

function tickStep(pps: number): number {
  for (const s of [1, 2, 5, 10, 15, 30, 60, 120, 300]) if (s * pps >= 70) return s;
  return 600;
}

/** CapCut-style timeline: ruler, video / subtitle / voice tracks, draggable playhead, zoom. */
export function Timeline({ projectId, lang, scenes, projectScenes, states, timeMs, totalMs, onSeek }: Props) {
  const { t } = useI18n();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [viewW, setViewW] = useState(800);
  const [zoom, setZoom] = useState<number | null>(null); // null = fit
  const dragging = useRef(false);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setViewW(el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const totalS = Math.max(1, totalMs / 1000);
  const fit = Math.min(MAX_PPS, Math.max(MIN_PPS, (viewW - LABEL_W - 24) / totalS));
  const pps = zoom ?? fit;
  const width = totalS * pps;
  const cues = useMemo(() => cuesOf(scenes, states), [scenes, states]);
  const thumbVersion = useMemo(() => {
    const m = new Map<string, string | null>();
    for (const s of projectScenes) m.set(s.id, s.image.thumb_path ? `${s.image.thumb_path}#${s.image.prompt_hash ?? ""}` : null);
    return m;
  }, [projectScenes]);
  const current = sceneIndexAt(scenes, timeMs);
  const x = (ms: number) => (ms / 1000) * pps;

  // Follow the playhead: during playback and after a seek (scene buttons, keys, clicks),
  // scroll so it stays visible. Runs only when the time or zoom changes, so scrolling the
  // timeline by hand while paused is not undone.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || dragging.current) return;
    const next = followScroll(LABEL_W + (timeMs / 1000) * pps, el.scrollLeft, el.clientWidth, LABEL_W);
    if (next !== null) el.scrollLeft = next;
  }, [timeMs, pps]);

  const seekFrom = (e: ReactPointerEvent<HTMLDivElement>) => {
    const el = scrollRef.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const px = e.clientX - rect.left + el.scrollLeft - LABEL_W;
    onSeek(Math.min(totalMs, Math.max(0, (px / pps) * 1000)));
  };

  const step = tickStep(pps);
  const ticks = Array.from({ length: Math.floor(totalS / step) + 1 }, (_, i) => i * step);

  return (
    <div className="tl">
      <div className="tl__toolbar">
        <span className="t-small t-muted">{t("editor.timeline")}</span>
        <div className="row">
          <IconButton label={t("editor.zoomOut")} icon={<ZoomOut {...ICON} />} onClick={() => setZoom(Math.max(MIN_PPS, pps / 1.5))} />
          <IconButton label={t("editor.zoomIn")} icon={<ZoomIn {...ICON} />} onClick={() => setZoom(Math.min(MAX_PPS, pps * 1.5))} />
          <IconButton label={t("editor.zoomFit")} icon={<Maximize2 {...ICON} />} onClick={() => setZoom(null)} />
        </div>
      </div>
      <div
        ref={scrollRef}
        className="tl__scroll"
        onPointerDown={(e) => {
          if ((e.target as HTMLElement).closest(".tl__labels")) return;
          dragging.current = true;
          e.currentTarget.setPointerCapture(e.pointerId);
          seekFrom(e);
        }}
        onPointerMove={(e) => dragging.current && seekFrom(e)}
        onPointerUp={() => {
          dragging.current = false;
        }}
      >
        <div className="tl__content" style={{ width: width + LABEL_W + 24 }}>
          <div className="tl__labels" aria-hidden="true">
            <span className="tl__label tl__label--ruler" />
            <span className="tl__label tl__label--video" title={t("editor.trackVideo")}>
              <ImageIcon {...ICON} />
            </span>
            <span className="tl__label tl__label--subs" title={t("editor.trackSubs")}>
              <Captions {...ICON} />
            </span>
            <span className="tl__label tl__label--audio" title={t("editor.trackVoice")}>
              <Volume2 {...ICON} />
            </span>
          </div>
          <div className="tl__tracks" style={{ left: LABEL_W, width }}>
            <div className="tl__ruler">
              {ticks.map((s) => (
                <span key={s} className="tl__tick" style={{ left: x(s * 1000) }}>
                  {formatTime(s * 1000)}
                </span>
              ))}
            </div>
            <div className="tl__track tl__track--video">
              {scenes.map((s, i) => (
                <div
                  key={s.scene_id}
                  className={`tl__clip${i === current ? " tl__clip--current" : ""}${s.image_ready ? "" : " tl__clip--missing"}`}
                  style={{ left: x(s.start_ms), width: Math.max(2, x(s.clip_ms) - 2) }}
                  title={`${t("script.scene", { n: s.order })} · ${(s.clip_ms / 1000).toFixed(1)} s`}
                >
                  {x(s.clip_ms) > MIN_THUMB_PX && (
                    <SceneThumb
                      projectId={projectId}
                      sceneId={s.scene_id}
                      version={thumbVersion.get(s.scene_id) ?? null}
                      alt=""
                    />
                  )}
                  <span className="tl__clip-num">{s.order}</span>
                </div>
              ))}
            </div>
            <div className="tl__track tl__track--subs">
              {cues.map((c) => (
                <div
                  key={c.start_ms}
                  className="tl__cue"
                  style={{ left: x(c.start_ms), width: Math.max(2, x(c.end_ms - c.start_ms) - 2) }}
                  title={c.text}
                  lang={lang}
                >
                  {x(c.end_ms - c.start_ms) > 30 && c.text}
                </div>
              ))}
            </div>
            <div className="tl__track tl__track--audio">
              {scenes.map((s) =>
                s.voice_ready ? (
                  <div
                    key={s.scene_id}
                    className="tl__voice"
                    style={{ left: x(s.start_ms), width: Math.max(2, x(s.voice_ms) - 1) }}
                  />
                ) : null,
              )}
            </div>
            <div className="tl__playhead" style={{ left: x(timeMs) }} aria-hidden="true" />
          </div>
        </div>
      </div>
    </div>
  );
}
