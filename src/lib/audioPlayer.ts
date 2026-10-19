// One shared <audio> player for voice playback: starting a new clip stops the previous one.

type Listener = (playingKey: string | null) => void;
type TimeListener = (time: { currentTime: number; duration: number }) => void;

let audio: HTMLAudioElement | null = null;
let objectUrl: string | null = null;
let current: string | null = null;
const listeners = new Set<Listener>();
const timeListeners = new Set<TimeListener>();

function notify() {
  for (const l of listeners) l(current);
}

function notifyTime() {
  if (!audio) return;
  const time = {
    currentTime: audio.currentTime || 0,
    duration: isFinite(audio.duration) ? audio.duration : 0,
  };
  for (const l of timeListeners) l(time);
}

function release() {
  if (objectUrl) URL.revokeObjectURL(objectUrl);
  objectUrl = null;
}

export function stopAudio() {
  audio?.pause();
  release();
  current = null;
  notify();
  notifyTime();
}

export function pauseAudio() {
  audio?.pause();
  notify();
}

export async function resumeAudio(): Promise<void> {
  if (audio && current && audio.paused) {
    await audio.play();
    notify();
  }
}

export function isAudioPaused(): boolean {
  return audio ? audio.paused : true;
}

export function getCurrentKey(): string | null {
  return current;
}

export function seekAudio(seconds: number) {
  if (audio && isFinite(seconds)) {
    audio.currentTime = seconds;
    notifyTime();
  }
}

export function getCurrentAudioTime(): { currentTime: number; duration: number } {
  return {
    currentTime: audio?.currentTime || 0,
    duration: audio && isFinite(audio.duration) ? audio.duration : 0,
  };
}

/** Plays raw MP3/WAV bytes. `key` identifies the clip (e.g. scene id) for UI state. */
export async function playAudio(
  key: string,
  bytes: ArrayBuffer,
  mime: string,
  onEnded?: () => void,
): Promise<void> {
  stopAudio();
  objectUrl = URL.createObjectURL(new Blob([bytes], { type: mime }));
  if (!audio) {
    audio = new Audio();
    audio.ontimeupdate = () => notifyTime();
  }
  audio.onended = () => {
    release();
    current = null;
    notify();
    notifyTime();
    onEnded?.();
  };
  audio.src = objectUrl;
  current = key;
  notify();
  notifyTime();
  try {
    await audio.play();
  } catch (e) {
    stopAudio();
    throw e;
  }
}

export function onPlayingChange(l: Listener): () => void {
  listeners.add(l);
  return () => listeners.delete(l);
}

export function onAudioTimeUpdate(l: TimeListener): () => void {
  timeListeners.add(l);
  return () => timeListeners.delete(l);
}

/** MIME type from a voice path (`…/voice.mp3` → audio/mpeg). */
export function mimeForPath(path: string | null | undefined): string {
  return path?.toLowerCase().endsWith(".wav") ? "audio/wav" : "audio/mpeg";
}

