// One shared <audio> player for voice playback: starting a new clip stops the previous one.

type Listener = (playingKey: string | null) => void;

let audio: HTMLAudioElement | null = null;
let objectUrl: string | null = null;
let current: string | null = null;
const listeners = new Set<Listener>();

function notify() {
  for (const l of listeners) l(current);
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
}

/** Plays raw MP3/WAV bytes. `key` identifies the clip (e.g. scene id) for UI state. */
export async function playAudio(key: string, bytes: ArrayBuffer, mime: string): Promise<void> {
  stopAudio();
  objectUrl = URL.createObjectURL(new Blob([bytes], { type: mime }));
  audio ??= new Audio();
  audio.onended = () => {
    release();
    current = null;
    notify();
  };
  audio.src = objectUrl;
  current = key;
  notify();
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

/** MIME type from a voice path (`…/voice.mp3` → audio/mpeg). */
export function mimeForPath(path: string | null | undefined): string {
  return path?.toLowerCase().endsWith(".wav") ? "audio/wav" : "audio/mpeg";
}
