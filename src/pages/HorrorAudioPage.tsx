import {
  Download,
  Headphones,
  Mic,
  Play,
  RotateCcw,
  Sparkles,
  Square,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import { Button } from "../components/Button";
import { PageHeader } from "../components/PageHeader";
import { useToast } from "../components/toast";
import { useI18n } from "../i18n/context";
import { api } from "../lib/api";
import type { Navigate } from "../lib/route";
import type { VoicesStatus } from "../lib/types";

interface TimelineSegment {
  id: string;
  segment_type: "speech" | "pause" | "sfx" | "ambience";
  speaker?: string;
  character_id?: string;
  text?: string;
  emotion?: string;
  duration_ms: number;
  sfx_id?: string;
  sfx_name?: string;
  pan?: number;
  volume?: number;
  ambience_action?: string;
}

interface CharacterVoice {
  id: string;
  name: string;
  role: string;
  voice_model: string;
  pitch: number;
  rate: number;
  reverb: number;
  color: string;
}

const DEFAULT_SCRIPT = `ចំណងជើងរឿង៖ «សំឡេងខ្សឹបក្នុងផ្ទះបុរាណ» (Whispers in the Old Manor)
ប្រភេទ៖ រឿងខ្មោចបែបស៊ើបអង្កេត និងចិត្តសាស្ត្រ (Horror Audio Drama)

ឈុតទី ១៖ ការមកដល់ និងការរៀបរាប់បរិយាកាសទីតាំងដ៏អាថ៌កំបាំង
[AMBIENCE: rain_storm_loop]
[SFX: rain]

អ្នកនិទានរឿង (Narrator)៖ ដំណើររឿងចាប់ផ្ដើមដោយសំឡេងម៉ាស៊ីនឡានចាស់មួយគ្រឿង បើកកាត់ផ្លូវគ្រួសក្រហម ក្រោមដំណក់ទឹកភ្លៀងរលឹមៗនាពេលរសៀលជ្រេ។ [pause: 1.5s] រិទ្ធី កំពុងតេស្តម៉ៃក្រូហ្វូនរបស់គាត់ ដោយនិយាយរៀបរាប់ពីកាលបរិច្ឆេទ និងទីតាំង ខណៈដែល សុជាតា អានឯកសារចាស់ៗអំពីប្រវត្តិនៃ «ភូមិគ្រឹះត្រកូលឧកញ៉ាចាស់»។

រិទ្ធី (Sound Engineer)៖ ម៉ៃក្រូហ្វូនឆានែលទីមួយ និងទីពីរ... ដំណើរការធម្មតា។ [pause: 0.8s] សុជាតា នាងប្រាកដចិត្តទេថា យើងត្រូវចុះទៅថតនៅម៉ោងនេះ? មេឃមើលទៅដូចចង់មានព្យុះធំហើយ។

សុជាតា (Researcher)៖ ឯកសារបានបញ្ជាក់ច្បាស់ណាស់ រិទ្ធី... រៀងរាល់យប់ថ្ងៃសីល តែងតែមានអ្នកភូមិឮសំឡេងឧបករណ៍ភ្លេងបុរាណបន្លឺចេញពីផ្ទះនេះ... [pause: 1.2s] [whisper] ទោះបីជាផ្ទះនេះ គ្មានមនុស្សរស់នៅជាង ៥០ ឆ្នាំហើយក៏ដោយ។

[SFX: thunder]

ឈុតទី ២៖ ការជួបអ្នកយាម និងការចូលទៅក្នុងភូមិគ្រឹះ
[SFX: creaking_door]

អ៊ំសាន (Caretaker)៖ អ្នកទាំងពីរ... [pause: 1.0s] [trembling] ប្រសិនបើមិនទាន់ហួសពេលទេ សូមត្រឡប់ទៅវិញទៅ។ ផ្ទះនេះ... វាមិនចូលចិត្តមនុស្សចម្លែកទេ។

រិទ្ធី (Sound Engineer)៖ ពួកយើងគ្រាន់តែមកកត់ត្រាសំឡេងបរិយាកាស និងស្ថាបត្យកម្មទេ អ៊ំ។ មិនមានចេតនាអាក្រក់ឡើយ។

អ៊ំសាន (Caretaker)៖ បើចង់ចូល... [pause: 1.5s] កុំប៉ះពាល់ឧបករណ៍ភ្លេងនៅជាន់លើឱ្យសោះ។ ហើយបើឮសំឡេងខ្សឹប... [whisper] ដាច់ខាតកុំឆ្លើយតប!

ឈុតទី ៣៖ ភាពស្ងប់ស្ងាត់ដ៏ព្រឺព្រួច និងការប្រទះឃើញបន្ទប់ភ្លេង
[SFX: slow_door_squeak]
[pause: 2.0s]

អ្នកនិទានរឿង (Narrator)៖ ខាងក្នុងភូមិគ្រឹះ គឺត្រជាក់ស្រេង ខុសពីបរិយាកាសខាងក្រៅ។ សំឡេងដំណក់ទឹកភ្លៀងស្រក់ប៉ះកម្រាលឈើលាន់ឮរណ្ដំ។ [pause: 1.2s] នៅចំកណ្តាលបន្ទប់ធំ មានតម្កល់ទ្រអ៊ូបុរាណមួយ ដែលគ្របដណ្តប់ដោយធូលីដីក្រាស់ឃ្មឹក។

[SFX: wooden_floor_creak]
រិទ្ធី (Sound Engineer)៖ សុជាតា... [pause: 1.0s] ម៉ាស៊ីនថតសំឡេងខ្ញុំ ចាប់បានរលកសញ្ញាចម្លែកមួយ... [whisper] ដូចជាមានសំឡេងដកដង្ហើមមនុស្សនៅក្បែរត្រចៀកខ្ញុំ!

[SFX: tro_string_snap]
[pause: 1.5s]

សុជាតា (Researcher)៖ [screaming] រិទ្ធី! ប្រយ័ត្ន! ខ្សែទ្រនោះ... វាដាច់ដោយខ្លួនឯង!

វិញ្ញាណម្ចាស់ផ្ទះ (Spirit)៖ [whisper] ...ឯងមកយកវាទៅណា?... នៅទីនេះជាមួយគ្នាទៅ...

[SFX: violent_door_slam]
[SFX: sub_bass_drone]`;

function parseHorrorScript(text: string) {
  const lines = text.split("\n").map((l) => l.trim()).filter((l) => l.length > 0);
  const newSegs: TimelineSegment[] = [];
  const charMap = new Map<string, CharacterVoice>();

  let currentSpeaker: string | null = null;
  let activeEmotion: string | null = null;

  for (const line of lines) {
    if (line.includes("ចំណងជើង") || line.startsWith("##") || line.startsWith("ឈុតទី") || line.startsWith("គោលដៅ") || line.startsWith("ប្រភេទ")) {
      continue;
    }

    if (line.startsWith("[AMBIENCE:")) {
      const val = line.replace(/\[AMBIENCE:/i, "").replace(/\]/, "").trim();
      const isStop = val.toLowerCase() === "stop";
      newSegs.push({
        id: `seg_${newSegs.length + 1}`,
        segment_type: "ambience",
        duration_ms: 0,
        sfx_name: val,
        ambience_action: isStop ? "stop" : "start",
      });
      continue;
    }

    if (line.startsWith("[SFX:")) {
      const val = line.replace(/\[SFX:/i, "").replace(/\]/, "").trim();
      newSegs.push({
        id: `seg_${newSegs.length + 1}`,
        segment_type: "sfx",
        duration_ms: 2200,
        sfx_id: val,
        sfx_name: val,
        pan: val.includes("left") ? -0.6 : val.includes("right") ? 0.6 : 0,
        volume: 0.8,
      });
      continue;
    }

    if (line.startsWith("[pause:")) {
      const val = line.replace(/\[pause:/i, "").replace(/\]/, "").trim();
      const sec = parseFloat(val.replace("s", "")) || 1.0;
      newSegs.push({
        id: `seg_${newSegs.length + 1}`,
        segment_type: "pause",
        duration_ms: Math.round(sec * 1000),
      });
      continue;
    }

    let speakerPart: string | null = null;
    let textPart = line;

    const colonIdx = line.indexOf("៖") !== -1 ? line.indexOf("៖") : line.indexOf(":");
    if (colonIdx !== -1 && colonIdx < 40) {
      speakerPart = line.substring(0, colonIdx).trim();
      textPart = line.substring(colonIdx + 1).trim();
    }

    if (speakerPart) {
      currentSpeaker = speakerPart;
    }

    const effectiveSpeaker = currentSpeaker || "អ្នកនិទានរឿង (Narrator)";

    if (!charMap.has(effectiveSpeaker)) {
      let voiceModel = "VoxCPM2 (Local)";
      let color = "var(--c-accent)";
      let pitch = 1.0;
      let rate = 1.0;

      if (effectiveSpeaker.includes("អ្នកនិទាន")) {
        voiceModel = "VoxCPM2 Narrator Deep";
        color = "#6366f1";
        pitch = 0.9;
        rate = 0.88;
      } else if (effectiveSpeaker.includes("រិទ្ធី")) {
        voiceModel = "VoxCPM2 Male Tense";
        color = "#0ea5e9";
        pitch = 1.0;
        rate = 0.95;
      } else if (effectiveSpeaker.includes("សុជាតា")) {
        voiceModel = "VoxCPM2 Female Tremble";
        color = "#ec4899";
        pitch = 1.05;
        rate = 0.92;
      } else if (effectiveSpeaker.includes("អ៊ំសាន")) {
        voiceModel = "VoxCPM2 Raspy Elder";
        color = "#d97706";
        pitch = 0.8;
        rate = 0.8;
      } else if (effectiveSpeaker.includes("វិញ្ញាណ") || effectiveSpeaker.includes("ខ្មោច")) {
        voiceModel = "VoxCPM2 Ghost Whisper";
        color = "#a855f7";
        pitch = 1.2;
        rate = 0.7;
      }

      charMap.set(effectiveSpeaker, {
        id: `char_${charMap.size + 1}`,
        name: effectiveSpeaker,
        role: "Character",
        voice_model: voiceModel,
        pitch,
        rate,
        reverb: 0.25,
        color,
      });
    }

    const words = textPart.split(" ");
    for (const word of words) {
      if (word.includes("[whisper]")) activeEmotion = "whisper";
      if (word.includes("[trembling]")) activeEmotion = "trembling";
      if (word.includes("[screaming]")) activeEmotion = "screaming";
    }

    const cleanText = textPart.replace(/\[.*?\]/g, "").trim();
    if (cleanText.length > 0) {
      const estMs = Math.max(1200, cleanText.length * 80);
      newSegs.push({
        id: `seg_${newSegs.length + 1}`,
        segment_type: "speech",
        speaker: effectiveSpeaker,
        character_id: charMap.get(effectiveSpeaker)?.id,
        text: cleanText,
        emotion: activeEmotion || undefined,
        duration_ms: estMs,
        pan: 0,
        volume: 1.0,
      });
    }
  }

  return { segments: newSegs, characters: Array.from(charMap.values()) };
}

interface Props {
  navigate: Navigate;
}

export function HorrorAudioPage({ navigate }: Props) {
  const { t } = useI18n();
  const toast = useToast();
  const [scriptText, setScriptText] = useState(DEFAULT_SCRIPT);
  const [parsedData, setParsedData] = useState(() => parseHorrorScript(DEFAULT_SCRIPT));
  const [activeTab, setActiveTab] = useState<"timeline" | "editor" | "cast" | "sfx">("timeline");
  const [isPlaying, setIsPlaying] = useState(false);
  const [currentIdx, setCurrentIdx] = useState(-1);
  const [isAmbiencePlaying, setIsAmbiencePlaying] = useState(false);
  const [voxStatus, setVoxStatus] = useState<VoicesStatus | null>(null);

  const { segments, characters } = parsedData;

  const audioCtxRef = useRef<AudioContext | null>(null);
  const ambienceRef = useRef<AudioNode | null>(null);
  const playbackTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const playSegmentRef = useRef<(idx: number) => void>(() => {});

  const getAudioContext = useCallback(() => {
    if (!audioCtxRef.current) {
      const AudioCtx = window.AudioContext || (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext;
      if (AudioCtx) audioCtxRef.current = new AudioCtx();
    }
    if (audioCtxRef.current && audioCtxRef.current.state === "suspended") {
      void audioCtxRef.current.resume();
    }
    return audioCtxRef.current;
  }, []);

  useEffect(() => {
    api.voicesStatus().then(setVoxStatus).catch(() => setVoxStatus(null));
  }, []);

  const playSynthesizedSfx = useCallback((sfxId: string, panVal = 0, vol = 0.8) => {
    const ctx = getAudioContext();
    if (!ctx) return;

    const masterGain = ctx.createGain();
    masterGain.gain.setValueAtTime(vol, ctx.currentTime);

    if (ctx.createStereoPanner) {
      const panner = ctx.createStereoPanner();
      panner.pan.setValueAtTime(panVal, ctx.currentTime);
      masterGain.connect(panner);
      panner.connect(ctx.destination);
    } else {
      masterGain.connect(ctx.destination);
    }

    if (sfxId.includes("tro") || sfxId.includes("snap")) {
      const osc = ctx.createOscillator();
      const oscGain = ctx.createGain();
      osc.type = "triangle";
      osc.frequency.setValueAtTime(580, ctx.currentTime);
      osc.frequency.exponentialRampToValueAtTime(140, ctx.currentTime + 0.5);
      oscGain.gain.setValueAtTime(0.9, ctx.currentTime);
      oscGain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 0.6);
      osc.connect(oscGain);
      oscGain.connect(masterGain);
      osc.start();
      osc.stop(ctx.currentTime + 0.65);
    } else if (sfxId.includes("thunder") || sfxId.includes("slam")) {
      const osc = ctx.createOscillator();
      const oscGain = ctx.createGain();
      osc.type = "sawtooth";
      osc.frequency.setValueAtTime(95, ctx.currentTime);
      osc.frequency.exponentialRampToValueAtTime(25, ctx.currentTime + 1.8);
      oscGain.gain.setValueAtTime(0.95, ctx.currentTime);
      oscGain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 2.0);
      osc.connect(oscGain);
      oscGain.connect(masterGain);
      osc.start();
      osc.stop(ctx.currentTime + 2.1);
    } else if (sfxId.includes("creak") || sfxId.includes("door")) {
      const osc = ctx.createOscillator();
      const oscGain = ctx.createGain();
      osc.type = "sawtooth";
      osc.frequency.setValueAtTime(210, ctx.currentTime);
      osc.frequency.linearRampToValueAtTime(420, ctx.currentTime + 0.4);
      osc.frequency.linearRampToValueAtTime(260, ctx.currentTime + 0.8);
      oscGain.gain.setValueAtTime(0.4, ctx.currentTime);
      oscGain.gain.exponentialRampToValueAtTime(0.001, ctx.currentTime + 1.0);
      osc.connect(oscGain);
      oscGain.connect(masterGain);
      osc.start();
      osc.stop(ctx.currentTime + 1.1);
    } else {
      const osc = ctx.createOscillator();
      const oscGain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.setValueAtTime(36, ctx.currentTime);
      osc.frequency.linearRampToValueAtTime(30, ctx.currentTime + 3.5);
      oscGain.gain.setValueAtTime(0.65, ctx.currentTime);
      oscGain.gain.linearRampToValueAtTime(0.01, ctx.currentTime + 4.0);
      osc.connect(oscGain);
      oscGain.connect(masterGain);
      osc.start();
      osc.stop(ctx.currentTime + 4.1);
    }
  }, [getAudioContext]);

  const toggleAmbience = useCallback(() => {
    const ctx = getAudioContext();
    if (!ctx) return;

    if (isAmbiencePlaying) {
      if (ambienceRef.current) {
        try {
          (ambienceRef.current as unknown as { stop: () => void }).stop();
        } catch {
          // ignore error
        }
        ambienceRef.current = null;
      }
      setIsAmbiencePlaying(false);
    } else {
      const bufferSize = ctx.sampleRate * 2;
      const buffer = ctx.createBuffer(1, bufferSize, ctx.sampleRate);
      const data = buffer.getChannelData(0);
      let b0 = 0;
      let b1 = 0;
      let b2 = 0;
      for (let i = 0; i < bufferSize; i++) {
        const white = Math.random() * 2 - 1;
        b0 = 0.99886 * b0 + white * 0.0555179;
        b1 = 0.99332 * b1 + white * 0.0750759;
        b2 = 0.969 * b2 + white * 0.153852;
        data[i] = (b0 + b1 + b2 + white * 0.5) * 0.035;
      }
      const noise = ctx.createBufferSource();
      noise.buffer = buffer;
      noise.loop = true;
      const filter = ctx.createBiquadFilter();
      filter.type = "lowpass";
      filter.frequency.setValueAtTime(900, ctx.currentTime);
      const gain = ctx.createGain();
      gain.gain.setValueAtTime(0.3, ctx.currentTime);
      noise.connect(filter);
      filter.connect(gain);
      gain.connect(ctx.destination);
      noise.start();
      ambienceRef.current = noise as unknown as AudioNode;
      setIsAmbiencePlaying(true);
    }
  }, [getAudioContext, isAmbiencePlaying]);

  useEffect(() => {
    playSegmentRef.current = (index: number) => {
      if (index >= segments.length) {
        setIsPlaying(false);
        setCurrentIdx(-1);
        return;
      }

      setCurrentIdx(index);
      const seg = segments[index];
      if (!seg) return;

      if (seg.segment_type === "pause") {
        playbackTimerRef.current = setTimeout(() => {
          playSegmentRef.current(index + 1);
        }, seg.duration_ms || 1000);
      } else if (seg.segment_type === "sfx") {
        playSynthesizedSfx(seg.sfx_id || "", seg.pan || 0, seg.volume || 0.8);
        playbackTimerRef.current = setTimeout(() => {
          playSegmentRef.current(index + 1);
        }, Math.min(seg.duration_ms || 2000, 2400));
      } else if (seg.segment_type === "ambience") {
        if (seg.ambience_action === "start" && !isAmbiencePlaying) {
          toggleAmbience();
        }
        playbackTimerRef.current = setTimeout(() => {
          playSegmentRef.current(index + 1);
        }, 200);
      } else if (seg.segment_type === "speech") {
        if (typeof window !== "undefined" && window.speechSynthesis) {
          window.speechSynthesis.cancel();
          const ut = new SpeechSynthesisUtterance(seg.text || "");
          const charConfig = characters.find((c) => c.name === seg.speaker);
          if (charConfig) {
            ut.pitch = charConfig.pitch;
            ut.rate = charConfig.rate;
          }
          if (seg.emotion === "whisper") {
            ut.volume = 0.5;
            ut.pitch = (charConfig?.pitch || 1.0) * 1.15;
          }
          ut.onend = () => {
            playbackTimerRef.current = setTimeout(() => {
              playSegmentRef.current(index + 1);
            }, 300);
          };
          ut.onerror = () => {
            playbackTimerRef.current = setTimeout(() => {
              playSegmentRef.current(index + 1);
            }, seg.duration_ms || 1500);
          };
          window.speechSynthesis.speak(ut);
        } else {
          playbackTimerRef.current = setTimeout(() => {
            playSegmentRef.current(index + 1);
          }, seg.duration_ms || 2000);
        }
      }
    };
  }, [segments, characters, isAmbiencePlaying, playSynthesizedSfx, toggleAmbience]);

  const startPlayback = () => {
    getAudioContext();
    setIsPlaying(true);
    playSegmentRef.current(0);
  };

  const stopPlayback = () => {
    setIsPlaying(false);
    setCurrentIdx(-1);
    if (playbackTimerRef.current) clearTimeout(playbackTimerRef.current);
    if (typeof window !== "undefined" && window.speechSynthesis) {
      window.speechSynthesis.cancel();
    }
  };

  const insertTag = (tag: string) => {
    setScriptText((prev) => prev + "\n" + tag);
    toast.show(`Inserted ${tag}`, "neutral");
  };

  const handleReParse = () => {
    setParsedData(parseHorrorScript(scriptText));
    toast.show("Script Re-Parsed", "ok");
  };

  return (
    <div className="page" style={{ padding: "24px 32px", display: "flex", flexDirection: "column", gap: "20px" }}>
      <PageHeader
        breadcrumb={[{ label: "Nitean Studio", onClick: () => navigate({ name: "projects" }) }, { label: t("nav.horrorAudio") }]}
        title={t("nav.horrorAudio")}
        meta={
          <p className="t-small t-muted">
            ផលិតរឿងខ្មោចព្រឺព្រួច (Pure Audio Drama) តាមរយៈ Vox CPM, Suspense Pauses, Sound Effects & Binaural Panning — គ្មានការ Generate រូបភាព
          </p>
        }
      />

      {/* Control Banner */}
      <div
        className="card"
        style={{
          padding: "16px 20px",
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          flexWrap: "wrap",
          gap: "12px",
          background: "var(--c-surface-card)",
          border: "1px solid var(--c-border)",
          borderRadius: "8px",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
          <div
            style={{
              width: "36px",
              height: "36px",
              borderRadius: "8px",
              background: "linear-gradient(135deg, #6366f1, #a855f7)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              color: "#fff",
            }}
          >
            <Headphones size={20} />
          </div>
          <div>
            <div style={{ fontSize: "14px", fontWeight: "bold", color: "var(--c-text)" }}>
              Acoustic Horror Drama Engine
            </div>
            <div style={{ fontSize: "11px", color: "var(--c-text-muted)" }}>
              {voxStatus?.profiles?.length
                ? `VoxCPM2 (${voxStatus.profiles.length} Profiles Found)`
                : "VoxCPM2 Engine Ready"} · 48kHz Stereo Master
            </div>
          </div>
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: "10px" }}>
          <Button
            variant="ghost"
            onClick={toggleAmbience}
            style={{ color: isAmbiencePlaying ? "#10b981" : "var(--c-text)" }}
          >
            {isAmbiencePlaying ? <Volume2 size={16} /> : <VolumeX size={16} />}
            Rain Ambience Bed: {isAmbiencePlaying ? "ON" : "OFF"}
          </Button>

          {isPlaying ? (
            <Button variant="danger" onClick={stopPlayback}>
              <Square size={16} />
              Stop Preview
            </Button>
          ) : (
            <Button variant="primary" onClick={startPlayback}>
              <Play size={16} />
              Play Full Timeline
            </Button>
          )}

          <Button
            variant="ghost"
            onClick={() => {
              const blob = new Blob([JSON.stringify({ script: scriptText, segments, characters }, null, 2)], {
                type: "application/json",
              });
              const url = URL.createObjectURL(blob);
              const a = document.createElement("a");
              a.href = url;
              a.download = "horror_audio_timeline.json";
              a.click();
              URL.revokeObjectURL(url);
            }}
          >
            <Download size={16} />
            Export
          </Button>
        </div>
      </div>

      {/* Tabs */}
      <div style={{ display: "flex", gap: "8px", borderBottom: "1px solid var(--c-border)" }}>
        <button
          className={`tab-item ${activeTab === "timeline" ? "tab-item--active" : ""}`}
          onClick={() => setActiveTab("timeline")}
          style={{
            padding: "8px 16px",
            background: "none",
            border: "none",
            borderBottom: activeTab === "timeline" ? "2px solid var(--c-accent)" : "2px solid transparent",
            color: activeTab === "timeline" ? "var(--c-accent)" : "var(--c-text-muted)",
            fontSize: "13px",
            fontWeight: 600,
            cursor: "pointer",
          }}
        >
          Timeline ({segments.length} Segments)
        </button>
        <button
          className={`tab-item ${activeTab === "editor" ? "tab-item--active" : ""}`}
          onClick={() => setActiveTab("editor")}
          style={{
            padding: "8px 16px",
            background: "none",
            border: "none",
            borderBottom: activeTab === "editor" ? "2px solid var(--c-accent)" : "2px solid transparent",
            color: activeTab === "editor" ? "var(--c-accent)" : "var(--c-text-muted)",
            fontSize: "13px",
            fontWeight: 600,
            cursor: "pointer",
          }}
        >
          Script Editor
        </button>
        <button
          className={`tab-item ${activeTab === "cast" ? "tab-item--active" : ""}`}
          onClick={() => setActiveTab("cast")}
          style={{
            padding: "8px 16px",
            background: "none",
            border: "none",
            borderBottom: activeTab === "cast" ? "2px solid var(--c-accent)" : "2px solid transparent",
            color: activeTab === "cast" ? "var(--c-accent)" : "var(--c-text-muted)",
            fontSize: "13px",
            fontWeight: 600,
            cursor: "pointer",
          }}
        >
          Vox CPM Cast ({characters.length})
        </button>
        <button
          className={`tab-item ${activeTab === "sfx" ? "tab-item--active" : ""}`}
          onClick={() => setActiveTab("sfx")}
          style={{
            padding: "8px 16px",
            background: "none",
            border: "none",
            borderBottom: activeTab === "sfx" ? "2px solid var(--c-accent)" : "2px solid transparent",
            color: activeTab === "sfx" ? "var(--c-accent)" : "var(--c-text-muted)",
            fontSize: "13px",
            fontWeight: 600,
            cursor: "pointer",
          }}
        >
          Horror SFX Soundboard
        </button>
      </div>

      {/* TAB CONTENT */}
      {activeTab === "timeline" && (
        <div style={{ display: "flex", flexDirection: "column", gap: "10px" }}>
          {segments.map((seg, idx) => {
            const isCur = currentIdx === idx;
            const char = characters.find((c) => c.name === seg.speaker);

            return (
              <div
                key={seg.id || idx}
                style={{
                  padding: "12px 16px",
                  borderRadius: "6px",
                  border: isCur ? "1px solid var(--c-accent)" : "1px solid var(--c-border)",
                  background: isCur ? "var(--c-accent-subtle, rgba(99, 102, 241, 0.1))" : "var(--c-surface-card)",
                  display: "flex",
                  flexDirection: "column",
                  gap: "6px",
                  transition: "all 0.15s ease",
                }}
              >
                <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                  <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                    <span style={{ fontSize: "11px", color: "var(--c-text-muted)", fontFamily: "monospace" }}>
                      #{idx + 1}
                    </span>
                    {seg.segment_type === "speech" && (
                      <span
                        style={{
                          fontSize: "12px",
                          fontWeight: "bold",
                          color: char?.color || "var(--c-accent)",
                          padding: "2px 8px",
                          borderRadius: "4px",
                          background: `${char?.color || "#6366f1"}15`,
                        }}
                      >
                        {seg.speaker}
                      </span>
                    )}
                    {seg.segment_type === "pause" && (
                      <span
                        style={{
                          fontSize: "12px",
                          fontWeight: "bold",
                          color: "#f59e0b",
                          padding: "2px 8px",
                          borderRadius: "4px",
                          background: "rgba(245, 158, 11, 0.15)",
                        }}
                      >
                        Suspense Pause: {seg.duration_ms / 1000}s
                      </span>
                    )}
                    {seg.segment_type === "sfx" && (
                      <span
                        style={{
                          fontSize: "12px",
                          fontWeight: "bold",
                          color: "#a855f7",
                          padding: "2px 8px",
                          borderRadius: "4px",
                          background: "rgba(168, 85, 247, 0.15)",
                        }}
                      >
                        SFX: {seg.sfx_name}
                      </span>
                    )}
                    {seg.segment_type === "ambience" && (
                      <span
                        style={{
                          fontSize: "12px",
                          fontWeight: "bold",
                          color: "#06b6d4",
                          padding: "2px 8px",
                          borderRadius: "4px",
                          background: "rgba(6, 182, 212, 0.15)",
                        }}
                      >
                        Ambience: {seg.sfx_name} ({seg.ambience_action?.toUpperCase()})
                      </span>
                    )}
                    {seg.emotion && (
                      <span style={{ fontSize: "11px", color: "var(--c-text-muted)", fontStyle: "italic" }}>
                        [{seg.emotion}]
                      </span>
                    )}
                  </div>

                  <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                    <span style={{ fontSize: "11px", color: "var(--c-text-muted)", fontFamily: "monospace" }}>
                      {(seg.duration_ms / 1000).toFixed(1)}s
                    </span>
                    <button
                      onClick={() => {
                        if (seg.segment_type === "sfx") {
                          playSynthesizedSfx(seg.sfx_id || "", seg.pan || 0, seg.volume || 0.8);
                        } else {
                          playSegmentRef.current(idx);
                        }
                      }}
                      style={{
                        padding: "4px 8px",
                        background: "var(--c-surface-inset)",
                        border: "1px solid var(--c-border)",
                        borderRadius: "4px",
                        color: "var(--c-text)",
                        cursor: "pointer",
                      }}
                    >
                      <Play size={12} />
                    </button>
                  </div>
                </div>

                {seg.text && (
                  <div style={{ fontSize: "13px", color: "var(--c-text)", paddingLeft: "26px", lineHeight: "1.5" }}>
                    {seg.text}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {activeTab === "editor" && (
        <div style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: "8px",
              flexWrap: "wrap",
              padding: "10px",
              background: "var(--c-surface-card)",
              borderRadius: "6px",
              border: "1px solid var(--c-border)",
            }}
          >
            <span style={{ fontSize: "12px", color: "var(--c-text-muted)" }}>Quick Insert:</span>
            <Button variant="ghost" onClick={() => insertTag("[pause: 1.5s]")}>
              + [Pause: 1.5s]
            </Button>
            <Button variant="ghost" onClick={() => insertTag("[pause: 3.0s]")}>
              + [Pause: 3.0s]
            </Button>
            <Button variant="ghost" onClick={() => insertTag("[whisper]")}>
              + [Whisper]
            </Button>
            <Button variant="ghost" onClick={() => insertTag("[trembling]")}>
              + [Trembling]
            </Button>
            <Button variant="ghost" onClick={() => insertTag("[SFX: tro_string_snap]")}>
              + [SFX: Tro Snap]
            </Button>
            <Button variant="ghost" onClick={() => insertTag("[SFX: wooden_floor_creak]")}>
              + [SFX: Wood Creak]
            </Button>
            <Button variant="ghost" onClick={() => insertTag("[SFX: thunder]")}>
              + [SFX: Thunder]
            </Button>
          </div>

          <textarea
            value={scriptText}
            onChange={(e) => setScriptText(e.target.value)}
            rows={18}
            style={{
              width: "100%",
              padding: "16px",
              borderRadius: "8px",
              background: "var(--c-surface-inset)",
              border: "1px solid var(--c-border)",
              color: "var(--c-text)",
              fontFamily: "monospace",
              fontSize: "13px",
              lineHeight: "1.6",
              boxSizing: "border-box",
            }}
          />

          <div style={{ display: "flex", justifyContent: "space-between" }}>
            <Button
              variant="ghost"
              onClick={() => {
                setScriptText(DEFAULT_SCRIPT);
                setParsedData(parseHorrorScript(DEFAULT_SCRIPT));
              }}
            >
              <RotateCcw size={16} />
              Reset to Sample
            </Button>
            <Button variant="primary" onClick={handleReParse}>
              <Sparkles size={16} />
              Re-Parse Timeline
            </Button>
          </div>
        </div>
      )}

      {activeTab === "cast" && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))", gap: "16px" }}>
          {characters.map((char) => (
            <div
              key={char.id}
              style={{
                padding: "16px",
                borderRadius: "8px",
                border: "1px solid var(--c-border)",
                background: "var(--c-surface-card)",
                display: "flex",
                flexDirection: "column",
                gap: "12px",
              }}
            >
              <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
                <span
                  style={{
                    fontSize: "13px",
                    fontWeight: "bold",
                    color: char.color,
                    padding: "2px 8px",
                    borderRadius: "4px",
                    background: `${char.color}15`,
                  }}
                >
                  {char.name}
                </span>
                <span style={{ fontSize: "11px", color: "var(--c-text-muted)" }}>{char.voice_model}</span>
              </div>

              <div
                style={{
                  fontSize: "12px",
                  color: "var(--c-text-muted)",
                  display: "flex",
                  flexDirection: "column",
                  gap: "4px",
                }}
              >
                <div>Pitch: {char.pitch}x</div>
                <div>Speed: {char.rate}x</div>
                <div>Reverb: {Math.round(char.reverb * 100)}%</div>
              </div>

              <Button
                variant="ghost"
                onClick={() => {
                  if (typeof window !== "undefined" && window.speechSynthesis) {
                    window.speechSynthesis.cancel();
                    const ut = new SpeechSynthesisUtterance(`${char.name} កំពុងសាកល្បងសំឡេង Vox CPM។`);
                    ut.pitch = char.pitch;
                    ut.rate = char.rate;
                    window.speechSynthesis.speak(ut);
                  }
                }}
              >
                <Mic size={14} />
                Test Voice Sample
              </Button>
            </div>
          ))}
        </div>
      )}

      {activeTab === "sfx" && (
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fill, minmax(280px, 1fr))", gap: "12px" }}>
          {[
            { id: "sfx_rain", name: "ភ្លៀងរលឹមលើដំបូល (Rain)", desc: "សំឡេងភ្លៀងរលឹមធ្លាក់លើដំបូលផ្ទះបុរាណ" },
            { id: "sfx_thunder", name: "ផ្គរលាន់រងំ (Thunderclap)", desc: "សំឡេងផ្គរលាន់រំញ័រដីពីចម្ងាយ" },
            { id: "sfx_creak", name: "កម្រាលឈើលាន់ (Creaking Floor)", desc: "ស្នូរជើងដើរលើបន្ទះឈើចាស់ក្រោកៗ" },
            { id: "sfx_tro", name: "ដាច់ខ្សែទ្រអ៊ូ (Khmer Tro Snap)", desc: "សំឡេងដាច់ខ្សែទ្រអ៊ូបុរាណ TWANG!" },
            { id: "sfx_slam", name: "ទ្វារទះក្តាំង (Door Slam)", desc: "សំឡេងទះទ្វារផ្ទុះក្តាំងក្នុងភាពស្ងាត់" },
            { id: "sfx_drone", name: "រំញ័រ Sub-Bass 32Hz (Drone)", desc: "រំញ័រ infrasonic បង្កើតភាពតានតឹងព្រឺក្បាល" },
          ].map((item) => (
            <div
              key={item.id}
              style={{
                padding: "14px",
                borderRadius: "6px",
                border: "1px solid var(--c-border)",
                background: "var(--c-surface-card)",
                display: "flex",
                alignItems: "center",
                justifyContent: "space-between",
              }}
            >
              <div>
                <div style={{ fontSize: "13px", fontWeight: "bold", color: "var(--c-text)" }}>{item.name}</div>
                <div style={{ fontSize: "11px", color: "var(--c-text-muted)" }}>{item.desc}</div>
              </div>
              <button
                onClick={() => playSynthesizedSfx(item.id)}
                style={{
                  padding: "6px 10px",
                  background: "var(--c-surface-inset)",
                  border: "1px solid var(--c-border)",
                  borderRadius: "4px",
                  color: "var(--c-accent)",
                  cursor: "pointer",
                }}
              >
                <Play size={14} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
