import { ListCollapse, Pause, Play, SkipBack, SkipForward, Square, Volume2, X } from "lucide-react";
import { useCallback, useRef } from "react";

import { Badge } from "./Badge";
import { IconButton } from "./Button";
import { useI18n } from "../i18n/context";
import type { Language, Scene } from "../lib/types";

const ICON = { size: 16, strokeWidth: 1.5 } as const;

function formatDuration(ms: number | null): string {
  if (ms === null || isNaN(ms)) return "0:00";
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export interface VoicePlayerBarProps {
  currentScene: Scene | null;
  currentIndex: number;
  totalScenes: number;
  isPaused: boolean;
  currentTime: number;
  duration: number;
  totalVoiceMs: number;
  elapsedBeforeMs: number;
  language: Language;
  autoScroll: boolean;
  onToggleAutoScroll: () => void;
  onPlayPause: () => void;
  onNext: () => void;
  onPrev: () => void;
  onStop: () => void;
  onSeek: (time: number) => void;
}

export function VoicePlayerBar({
  currentScene,
  currentIndex,
  totalScenes,
  isPaused,
  currentTime,
  duration,
  totalVoiceMs,
  elapsedBeforeMs,
  language,
  autoScroll,
  onToggleAutoScroll,
  onPlayPause,
  onNext,
  onPrev,
  onStop,
  onSeek,
}: VoicePlayerBarProps) {
  const { t } = useI18n();
  const trackRef = useRef<HTMLDivElement>(null);

  const sceneDurationMs = duration > 0 ? duration * 1000 : (currentScene?.voice.duration_ms ?? 0);
  const currentSceneMs = currentTime * 1000;
  const currentTotalMs = elapsedBeforeMs + currentSceneMs;

  const handleTrackClick = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (!trackRef.current || duration <= 0) return;
      const rect = trackRef.current.getBoundingClientRect();
      const clickX = e.clientX - rect.left;
      const pct = Math.max(0, Math.min(1, clickX / rect.width));
      onSeek(pct * duration);
    },
    [duration, onSeek],
  );

  if (!currentScene) return null;

  const progressPercent = duration > 0 ? Math.min(100, Math.max(0, (currentTime / duration) * 100)) : 0;

  return (
    <div className="voice-player-bar card" role="region" aria-label={t("scenes.playerBar")}>
      <div className="voice-player-bar__row">
        <div className="voice-player-bar__controls">
          <IconButton
            label={t("scenes.prevScene")}
            icon={<SkipBack {...ICON} />}
            disabled={currentIndex <= 1}
            onClick={onPrev}
          />
          <IconButton
            label={isPaused ? t("scenes.resumeAll") : t("scenes.pauseAll")}
            icon={isPaused ? <Play {...ICON} /> : <Pause {...ICON} />}
            onClick={onPlayPause}
          />
          <IconButton
            label={t("scenes.nextScene")}
            icon={<SkipForward {...ICON} />}
            disabled={currentIndex >= totalScenes}
            onClick={onNext}
          />
          <IconButton
            label={t("scenes.stopAllVoices")}
            icon={<Square {...ICON} />}
            onClick={onStop}
          />
        </div>

        <div className="voice-player-bar__meta">
          <Badge>
            <Volume2 size={12} strokeWidth={1.5} className="mr-1 inline-block" />
            {t("scenes.continuousPlaying", { current: currentIndex, total: totalScenes })}
          </Badge>
          <span className="t-small t-muted">
            {formatDuration(currentSceneMs)} / {formatDuration(sceneDurationMs)}
          </span>
          <span className="t-small t-muted">
            ({t("scenes.totalVoiceTime", { time: `${formatDuration(currentTotalMs)} / ${formatDuration(totalVoiceMs)}` })})
          </span>
        </div>

        <div
          ref={trackRef}
          className="voice-player-bar__progress-track"
          onClick={handleTrackClick}
          title={t("scenes.seek")}
        >
          <div
            className="voice-player-bar__progress-fill"
            style={{ width: `${progressPercent}%` }}
          />
        </div>

        <div className="voice-player-bar__actions">
          <button
            type="button"
            className={`btn btn--pill t-small ${autoScroll ? "btn--selected" : ""}`}
            onClick={onToggleAutoScroll}
            title={t("scenes.autoScrollHint")}
          >
            <ListCollapse size={14} strokeWidth={1.5} />
            {t("scenes.autoScroll")}
          </button>
          <IconButton
            label={t("scenes.stopAllVoices")}
            icon={<X size={15} strokeWidth={1.5} />}
            onClick={onStop}
          />
        </div>
      </div>

      <p className="voice-player-bar__narration" lang={language}>
        {currentScene.narration}
      </p>
    </div>
  );
}
