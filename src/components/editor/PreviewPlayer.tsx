import { ChevronFirst, ChevronLast, LoaderCircle, Pause, Play } from "lucide-react";
import { useEffect, useRef, useState, type RefObject } from "react";

import { IconButton } from "../Button";
import { useI18n } from "../../i18n/context";
import { PreviewAssets } from "../../lib/previewAssets";
import { bandFor, ensureFont, paintCue } from "../../lib/subtitleCanvas";
import { stateAt, type SubtitleState } from "../../lib/subtitleStates";
import { formatTime, sceneIndexAt, totalMs } from "../../lib/timeline";
import type { TimelineScene } from "../../lib/types";

/** Preview canvas size; the export is 1920×1080. */
const W = 1280;
const H = 720;
const SCALE = W / 1920;
/** Gap under the subtitle band (24 px at 1080p, `render::clip::Frame`). */
const SUBS_MARGIN = 24 * SCALE;
/** Crop share of the pan-size image (1920 of 2150 px, 1080 of 1210 px). */
const CROP_W = 1920 / 2150;
const CROP_H = 1080 / 1210;
const EMIT_MS = 66;
const ICON = { size: 18, strokeWidth: 1.5 } as const;

export interface PlayerApi {
  seek: (ms: number) => void;
  toggle: () => void;
  pause: () => void;
  stepScene: (delta: number) => void;
}

interface Props {
  projectId: string;
  scenes: TimelineScene[];
  /** Subtitle states per scene (same order as `scenes`). */
  states: SubtitleState[][];
  highlight: string | null;
  resolution: string;
  audioMime: string;
  apiRef: RefObject<PlayerApi | null>;
  onTime: (ms: number) => void;
}

/** Pan crop origin in image pixels, same expressions as `render::clip::Pan`. */
function cropOrigin(pan: TimelineScene["pan"], iw: number, ih: number, cw: number, ch: number, p: number) {
  const mx = (iw - cw) / 2;
  const my = (ih - ch) / 2;
  switch (pan) {
    case "lr":
      return [(iw - cw) * p, my];
    case "rl":
      return [(iw - cw) * (1 - p), my];
    case "tb":
      return [mx, (ih - ch) * p];
    default:
      return [mx, (ih - ch) * (1 - p)];
  }
}

/** CapCut-style preview: image pan + voice + subtitles with the word tracker, drawn live. */
export function PreviewPlayer({ projectId, scenes, states, highlight, resolution, audioMime, apiRef, onTime }: Props) {
  const { t } = useI18n();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const assetsRef = useRef<PreviewAssets | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const live = useRef({ scenes, states, highlight, resolution, onTime });
  const clock = useRef({ t: 0, playing: false, lastNow: 0, synced: -1, dirty: true, buffering: false });
  const [playing, setPlaying] = useState(false);
  const [buffering, setBuffering] = useState(false);
  const [time, setTime] = useState(0);

  useEffect(() => {
    live.current = { scenes, states, highlight, resolution, onTime };
    clock.current.dirty = true;
  });

  useEffect(() => {
    const assets = new PreviewAssets(projectId, audioMime, () => {
      clock.current.dirty = true;
    });
    const audio = new Audio();
    assetsRef.current = assets;
    audioRef.current = audio;
    void ensureFont("ក", bandFor("1080p", SCALE))
      .catch(() => undefined)
      .then(() => {
        clock.current.dirty = true;
      });
    return () => {
      audio.pause();
      audio.removeAttribute("src");
      assets.dispose();
    };
  }, [projectId, audioMime]);

  // Imperative controls for the page (timeline seek, keyboard).
  useEffect(() => {
    const c = clock.current;
    const setPlay = (on: boolean) => {
      const { scenes: list } = live.current;
      if (on && c.t >= totalMs(list) - 50) c.t = 0;
      c.playing = on;
      c.lastNow = performance.now();
      c.synced = -1;
      c.dirty = true;
      if (!on) audioRef.current?.pause();
      setPlaying(on);
    };
    apiRef.current = {
      seek: (ms) => {
        c.t = Math.min(Math.max(0, ms), totalMs(live.current.scenes));
        c.synced = -1;
        c.lastNow = performance.now();
        c.dirty = true;
        audioRef.current?.pause();
      },
      toggle: () => setPlay(!c.playing),
      pause: () => setPlay(false),
      stepScene: (delta) => {
        const list = live.current.scenes;
        const i = sceneIndexAt(list, c.t);
        const target = list[Math.min(list.length - 1, Math.max(0, i + delta))];
        if (target) apiRef.current?.seek(target.start_ms);
      },
    };
    return () => {
      apiRef.current = null;
    };
  }, [apiRef]);

  // Render loop.
  useEffect(() => {
    let raf = 0;
    let lastEmit = 0;
    const frame = (now: number) => {
      raf = requestAnimationFrame(frame);
      const c = clock.current;
      const { scenes: list, states: subs, highlight: hl, resolution: res } = live.current;
      const assets = assetsRef.current;
      const audio = audioRef.current;
      const canvas = canvasRef.current;
      const ctx = canvas?.getContext("2d");
      if (!assets || !audio || !ctx || list.length === 0) return;
      const total = totalMs(list);
      const i = sceneIndexAt(list, c.t);
      const sc = list[i];
      if (!sc) return;

      assets.retain(new Set(list.slice(Math.max(0, i - 1), i + 3).map((s) => s.scene_id)));
      for (const s of list.slice(i + 1, i + 3)) assets.get(s.scene_id, s.image_ready, s.voice_ready);
      const entry = assets.get(sc.scene_id, sc.image_ready, sc.voice_ready);

      if (c.playing) {
        const ready = assets.ready(sc.scene_id, sc.image_ready, sc.voice_ready);
        if (ready !== !c.buffering) {
          c.buffering = !ready;
          setBuffering(!ready);
        }
        if (!ready) {
          c.lastNow = now;
          c.synced = -1;
          audio.pause();
        } else {
          const local = c.t - sc.start_ms;
          if (c.synced !== i) {
            c.synced = i;
            if (entry.audio && local < sc.voice_ms) {
              audio.src = entry.audio;
              audio.currentTime = local / 1000;
              void audio.play().catch(() => undefined);
            } else {
              audio.pause();
            }
          }
          const dt = now - c.lastNow;
          c.lastNow = now;
          // While the voice plays it drives the clock (subtitles stay in sync with it).
          if (!audio.paused && audio.readyState >= 2 && local < sc.voice_ms) {
            c.t = sc.start_ms + audio.currentTime * 1000;
          } else if (audio.paused || local >= sc.voice_ms) {
            c.t += dt;
          }
          if (c.t - sc.start_ms >= sc.voice_ms && !audio.paused) audio.pause();
          if (c.t >= total) {
            c.t = total;
            c.playing = false;
            audio.pause();
            setPlaying(false);
          }
          c.dirty = true;
        }
      }

      if (!c.dirty) return;
      c.dirty = false;
      const cur = list[sceneIndexAt(list, c.t)] ?? sc;
      const curEntry = cur === sc ? entry : assets.get(cur.scene_id, cur.image_ready, cur.voice_ready);
      const local = c.t - cur.start_ms;
      ctx.fillStyle = "#0b0b0b";
      ctx.fillRect(0, 0, W, H);
      const img = curEntry.image;
      if (img) {
        const cw = img.width * CROP_W;
        const ch = img.height * CROP_H;
        const p = Math.min(1, Math.max(0, local / cur.clip_ms));
        const [sx, sy] = cropOrigin(cur.pan, img.width, img.height, cw, ch, p);
        ctx.drawImage(img, sx ?? 0, sy ?? 0, cw, ch, 0, 0, W, H);
      } else {
        ctx.fillStyle = "#6f6e6a";
        ctx.font = "500 22px Manrope, sans-serif";
        ctx.textAlign = "center";
        ctx.textBaseline = "middle";
        ctx.fillText(cur.image_ready ? "…" : t("editor.noImage", { n: cur.order }), W / 2, H / 2);
      }
      const st = stateAt(subs[list.indexOf(cur)] ?? [], local);
      if (st) {
        const band = bandFor(res, SCALE);
        paintCue(ctx, st.lines, st.active, { highlight: hl }, band, H - band.height - SUBS_MARGIN);
      }
      if (now - lastEmit > EMIT_MS || !c.playing) {
        lastEmit = now;
        setTime(c.t);
        live.current.onTime(c.t);
      }
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [t]);

  const total = totalMs(scenes);
  return (
    <div className="player">
      <div className="player__stage">
        <canvas ref={canvasRef} width={W} height={H} className="player__canvas" aria-label={t("editor.preview")} />
        {buffering && (
          <div className="player__buffer" aria-live="polite">
            <LoaderCircle size={28} strokeWidth={1.5} className="spin" aria-label={t("common.loading")} />
          </div>
        )}
      </div>
      <div className="player__bar">
        <span className="player__time t-small">
          {formatTime(time)} <span className="t-muted">/ {formatTime(total)}</span>
        </span>
        <div className="player__controls">
          <IconButton label={t("editor.prevScene")} icon={<ChevronFirst {...ICON} />} onClick={() => apiRef.current?.stepScene(-1)} />
          <button
            type="button"
            className="player__play"
            aria-label={playing ? t("editor.pause") : t("editor.play")}
            title={`${playing ? t("editor.pause") : t("editor.play")} (Space)`}
            onClick={() => apiRef.current?.toggle()}
          >
            {playing ? <Pause size={20} strokeWidth={1.75} /> : <Play size={20} strokeWidth={1.75} />}
          </button>
          <IconButton label={t("editor.nextScene")} icon={<ChevronLast {...ICON} />} onClick={() => apiRef.current?.stepScene(1)} />
        </div>
        <span className="player__time t-small t-muted">1080p · 30 fps</span>
      </div>
    </div>
  );
}
