//! VoxCPM2 (local) voice engine (`docs/09-voice-profiles.md`), behind the same `TtsEngine`
//! trait as Edge and Gemini TTS. Per scene:
//! 1. split the spoken text into sentence-sized pieces (`chunker`),
//! 2. synthesize each piece by cloning the profile's reference clip (same seed → same voice),
//!    retrying with a new seed when the length is far off the text's expected length, and
//!    halving the piece when the GPU runs out of memory,
//! 3. trim each piece's silence, join with short pauses, normalise loudness,
//! 4. return one WAV plus one timing entry per piece (sentence-level word timings, used by the
//!    subtitle word tracker exactly like Edge's phrase timings).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::audio::{expected_ms, suspicious_length};
use super::chunker;
use super::voxcpm_server::{ServerSpec, SpeakRequest, VoxError, VoxServer};
use super::wav::{self, Pcm};
use super::{AudioFormat, TtsEngine, TtsError, TtsFuture, TtsOutput, TtsRequest, WordTiming};
use crate::gemini::retry::RetryHooks;
use crate::pipeline::project::Language;

/// Silence before the first sentence and between sentences.
pub const LEAD_MS: u64 = 80;
pub const GAP_MS: u64 = 220;
/// Original silence kept around each piece after trimming.
const TRIM_MARGIN_MS: u64 = 40;
/// Tries per piece when its length looks wrong (truncated / looping output).
const MAX_ATTEMPTS: u64 = 3;
/// How often a piece may be halved after running out of GPU memory.
const MAX_SPLIT_DEPTH: u32 = 3;
const TARGET_RMS_DBFS: f64 = -18.0;
const PEAK_DBFS: f64 = -1.0;
/// Fade at both ends of each piece (no click where it was trimmed).
const FADE_MS: u64 = 8;
/// Pause hiss cleanup after normalising (`wav::quiet_pauses`): speech sits around −18 dBFS,
/// the model's background hiss around −45…−55 dBFS.
const PAUSE_THRESHOLD_DBFS: f64 = -40.0;
const PAUSE_RATIO: f64 = 3.0;
const PAUSE_RANGE_DB: f64 = 24.0;
const PAUSE_RELEASE_MS: u64 = 30;

pub type SpeakFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, VoxError>> + Send + 'a>>;
/// Text + trimmed audio of each piece a chunk ended up as.
type PiecesFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<(String, Pcm)>, VoxError>> + Send + 'a>>;

/// Something that turns one request into WAV bytes (the real server, or a fake in tests).
pub trait Speaker: Send + Sync {
    fn speak<'a>(&'a self, req: &'a SpeakRequest, cancel: &'a CancellationToken) -> SpeakFuture<'a>;
}

/// The shared audio.cpp server; every request holds the app-wide GPU lock (image generation
/// and video export share the same GPU).
pub struct ServerSpeaker {
    pub server: Arc<VoxServer>,
    pub spec: ServerSpec,
    pub gpu_lock: Arc<Mutex<()>>,
}

impl Speaker for ServerSpeaker {
    fn speak<'a>(&'a self, req: &'a SpeakRequest, cancel: &'a CancellationToken) -> SpeakFuture<'a> {
        Box::pin(async move {
            let _gpu = tokio::select! {
                () = cancel.cancelled() => return Err(VoxError::Cancelled),
                g = self.gpu_lock.lock() => g,
            };
            self.server.speak(&self.spec, req, cancel).await
        })
    }
}

/// A Voice Profile resolved for synthesis.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedVoice {
    pub reference: PathBuf,
    /// Empty = plain cloning; else "ultimate" cloning with this transcript.
    pub transcript: String,
    pub style: String,
    pub cfg: f32,
    pub steps: u32,
}

pub struct VoxCpmTts {
    speaker: Arc<dyn Speaker>,
    voice: ResolvedVoice,
    language: Language,
    seed: u64,
}

fn to_tts(e: VoxError) -> TtsError {
    match e {
        VoxError::Cancelled => TtsError::Cancelled,
        VoxError::Missing(_) | VoxError::ModelMissing | VoxError::Crashed(_) => TtsError::LocalUnavailable(e.to_string()),
        VoxError::OutOfMemory(_) | VoxError::Http(_) => TtsError::Local(e.to_string()),
    }
}

impl VoxCpmTts {
    pub fn new(speaker: Arc<dyn Speaker>, voice: ResolvedVoice, language: Language, seed: u64) -> Self {
        Self { speaker, voice, language, seed }
    }

    fn request(&self, text: &str, attempt: u64) -> SpeakRequest {
        let style = self.voice.style.trim();
        SpeakRequest {
            text: if style.is_empty() { text.to_owned() } else { format!("({style}){text}") },
            seed: self.seed + attempt,
            steps: self.voice.steps,
            guidance: self.voice.cfg,
            voice_ref: Some(self.voice.reference.clone()),
            reference_text: Some(self.voice.transcript.clone()),
        }
    }

    /// One piece → trimmed audio (possibly several, if it had to be split).
    fn piece<'a>(&'a self, text: &'a str, depth: u32, cancel: &'a CancellationToken) -> PiecesFuture<'a> {
        Box::pin(async move {
            let expected = expected_ms(text, self.language, 0);
            let mut last = None;
            for attempt in 0..MAX_ATTEMPTS {
                match self.speaker.speak(&self.request(text, attempt), cancel).await {
                    Err(VoxError::OutOfMemory(m)) => {
                        let Some((a, b)) = chunker::halves(text).filter(|_| depth < MAX_SPLIT_DEPTH) else {
                            return Err(VoxError::OutOfMemory(m));
                        };
                        let mut out = self.piece(&a, depth + 1, cancel).await?;
                        out.extend(self.piece(&b, depth + 1, cancel).await?);
                        return Ok(out);
                    }
                    Err(e) => return Err(e),
                    Ok(bytes) => {
                        let pcm = wav::parse(&bytes).map_err(VoxError::Http)?;
                        let speech = wav::trim(&pcm, TRIM_MARGIN_MS);
                        if !speech.samples.is_empty() && !suspicious_length(wav::ms(&speech), expected) {
                            return Ok(vec![(text.to_owned(), speech)]);
                        }
                        last = Some(speech);
                    }
                }
            }
            // Still odd after every try: keep the last take; the voice job flags the scene
            // (`suspicious_length`) so the user can listen and regenerate.
            match last {
                Some(p) if !p.samples.is_empty() => Ok(vec![(text.to_owned(), p)]),
                _ => Err(VoxError::Http("the voice engine returned only silence".into())),
            }
        })
    }

    /// Synthesizes a whole scene text; normalised mono PCM + one timing per piece.
    pub async fn synth(&self, text: &str, cancel: &CancellationToken) -> Result<(Pcm, Vec<WordTiming>), TtsError> {
        let chunks = chunker::chunk(text, self.language);
        if chunks.is_empty() {
            return Err(TtsError::BadAudio("empty text".into()));
        }
        let mut out = Pcm { rate: 0, samples: Vec::new() };
        let mut words = Vec::new();
        for chunk in &chunks {
            if cancel.is_cancelled() {
                return Err(TtsError::Cancelled);
            }
            for (piece_text, mut p) in self.piece(chunk, 0, cancel).await.map_err(to_tts)? {
                wav::fade_edges(&mut p, FADE_MS);
                if out.rate == 0 {
                    out.rate = p.rate;
                    out.samples.extend(wav::silence(p.rate, LEAD_MS));
                } else if p.rate != out.rate {
                    return Err(TtsError::BadAudio(format!("sample rate changed ({} → {})", out.rate, p.rate)));
                } else {
                    out.samples.extend(wav::silence(out.rate, GAP_MS));
                }
                let start_ms = wav::ms(&out);
                out.samples.extend(p.samples);
                words.push(WordTiming { text: piece_text, start_ms, end_ms: wav::ms(&out) });
            }
        }
        wav::normalize(&mut out, TARGET_RMS_DBFS, PEAK_DBFS);
        wav::quiet_pauses(&mut out, PAUSE_THRESHOLD_DBFS, PAUSE_RATIO, PAUSE_RANGE_DB, PAUSE_RELEASE_MS);
        Ok((out, words))
    }
}

impl TtsEngine for VoxCpmTts {
    fn synthesize<'a>(&'a self, request: &'a TtsRequest, hooks: &'a RetryHooks) -> TtsFuture<'a> {
        Box::pin(async move {
            let (pcm, words) = self.synth(&request.text, &hooks.cancel).await?;
            Ok(TtsOutput { audio: wav::to_wav(&pcm), format: AudioFormat::Wav, words })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use super::*;
    use crate::tts::wav::tests::tone;

    /// Fake server: speech length ≈ 90 ms per grapheme (the Khmer expectation), with
    /// scripted failures.
    struct Fake {
        calls: StdMutex<Vec<SpeakRequest>>,
        /// Requests whose text is longer than this run out of memory.
        oom_over_chars: usize,
        /// The first N requests return far too short audio.
        short_first: usize,
    }

    impl Fake {
        fn new(oom_over_chars: usize, short_first: usize) -> Self {
            Self { calls: StdMutex::new(vec![]), oom_over_chars, short_first }
        }
    }

    impl Speaker for Fake {
        fn speak<'a>(&'a self, req: &'a SpeakRequest, _c: &'a CancellationToken) -> SpeakFuture<'a> {
            Box::pin(async move {
                let n = {
                    let mut c = self.calls.lock().unwrap();
                    c.push(req.clone());
                    c.len()
                };
                let body = req.text.rsplit(')').next().unwrap_or(&req.text);
                if body.chars().count() > self.oom_over_chars {
                    return Err(VoxError::OutOfMemory("failed to allocate".into()));
                }
                let ms = expected_ms(body, Language::Km, 0);
                let ms = if n <= self.short_first { ms / 10 } else { ms };
                Ok(wav::to_wav(&tone(48_000, 200, ms, 0.05)))
            })
        }
    }

    fn voice() -> ResolvedVoice {
        ResolvedVoice { reference: PathBuf::from("C:/v/ref.wav"), transcript: "ref text".into(), style: "calm".into(), cfg: 2.0, steps: 10 }
    }

    const KM: &str = "តើអ្នកដឹងទេថា ដើម្បីសាងសង់ប្រាសាទអង្គរវត្តដ៏អស្ចារ្យនេះ បុព្វបុរសខ្មែរយើងត្រូវការពេលតិចជាងសាមសិបឆ្នាំទៅទៀត? ដើម្បីបានថ្មភក់រាប់លានដុំមកសាងសង់ ដំណើរការដ៏ធំសម្បើមមួយត្រូវចាប់ផ្តើមឡើងនៅឯភ្នំគូលែន។";

    #[tokio::test]
    async fn one_request_per_sentence_with_profile_settings_and_sentence_timings() {
        let fake = Arc::new(Fake::new(10_000, 0));
        let tts = VoxCpmTts::new(fake.clone(), voice(), Language::Km, 42);
        let (pcm, words) = tts.synth(KM, &CancellationToken::new()).await.unwrap();
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert!(calls[0].text.starts_with("(calm)"));
        assert_eq!(calls[0].voice_ref, Some(PathBuf::from("C:/v/ref.wav")));
        assert_eq!(calls[0].reference_text.as_deref(), Some("ref text"));
        assert_eq!((calls[0].seed, calls[1].seed), (42, 42), "same seed for every sentence");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].start_ms, LEAD_MS);
        assert!(words[1].start_ms >= words[0].end_ms + GAP_MS - 1);
        assert_eq!(words[1].end_ms, wav::ms(&pcm));
        assert!(words[0].text.ends_with('?'));
    }

    #[tokio::test]
    async fn retries_with_a_new_seed_when_the_length_is_far_off() {
        let fake = Arc::new(Fake::new(10_000, 1));
        let tts = VoxCpmTts::new(fake.clone(), voice(), Language::Km, 7);
        tts.synth("សួស្តី អ្នកទាំងអស់គ្នា នេះជាការសាកល្បង។", &CancellationToken::new()).await.unwrap();
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!((calls[0].seed, calls[1].seed), (7, 8));
    }

    #[tokio::test]
    async fn out_of_memory_halves_the_sentence() {
        let fake = Arc::new(Fake::new(40, 0));
        let tts = VoxCpmTts::new(fake.clone(), voice(), Language::Km, 1);
        let (_, words) = tts.synth("ដើម្បីបានថ្មភក់រាប់លានដុំមកសាងសង់ ដំណើរការដ៏ធំសម្បើមមួយត្រូវចាប់ផ្តើម។", &CancellationToken::new()).await.unwrap();
        assert!(words.len() >= 2, "split into halves: {words:?}");
        assert_eq!(words.iter().map(|w| w.text.replace(' ', "")).collect::<String>(), "ដើម្បីបានថ្មភក់រាប់លានដុំមកសាងសង់ដំណើរការដ៏ធំសម្បើមមួយត្រូវចាប់ផ្តើម។");
    }

    #[tokio::test]
    async fn engine_failures_map_to_job_errors() {
        assert_eq!(to_tts(VoxError::ModelMissing).kind(), "local_voice_unavailable");
        assert_eq!(to_tts(VoxError::Crashed("x".into())).kind(), "local_voice_unavailable");
        assert_eq!(to_tts(VoxError::OutOfMemory("x".into())).kind(), "local_voice");
        assert_eq!(to_tts(VoxError::Cancelled).kind(), "cancelled");
    }

    #[tokio::test]
    async fn output_is_a_normalised_wav() {
        let fake = Arc::new(Fake::new(10_000, 0));
        let tts = VoxCpmTts::new(fake, voice(), Language::Km, 1);
        let hooks = RetryHooks::default();
        let req = TtsRequest { text: "សួស្តី អ្នកទាំងអស់គ្នា។".into(), voice: String::new(), rate: 0, pitch: 0, model: String::new(), style: String::new() };
        let out = tts.synthesize(&req, &hooks).await.unwrap();
        assert_eq!(out.format, AudioFormat::Wav);
        let pcm = wav::parse(&out.audio).unwrap();
        let peak = pcm.samples.iter().map(|v| i32::from(*v).abs()).max().unwrap();
        assert!(peak > 5_000, "quiet input raised towards -18 dBFS: {peak}");
    }

    /// Real engine, real GPU: `VOXCPM_MODEL` (gguf), `VOXCPM_REF` (16 kHz reference clip),
    /// `VOXCPM_OUT` (folder for the result), optional `VOXCPM_REPEAT`. Run with
    /// `--ignored --nocapture`.
    #[tokio::test]
    #[ignore = "needs the VoxCPM2 model and a GPU"]
    async fn real_engine_speaks_a_khmer_scene() {
        let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
        let out = PathBuf::from(var("VOXCPM_OUT"));
        std::fs::create_dir_all(&out).unwrap();
        let repetitions = std::env::var("VOXCPM_REPEAT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(1)
            .clamp(1, 32);
        let spec = ServerSpec {
            exe: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/audiocpp/audiocpp_server.exe"),
            model: PathBuf::from(var("VOXCPM_MODEL")),
            device: super::super::voxcpm_server::pick_device(
                &super::super::voxcpm_server::list_devices(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/audiocpp/audiocpp_server.exe")).await,
                None,
            ),
            work_dir: out.join("server"),
        };
        let server = Arc::new(VoxServer::default());
        let speaker = Arc::new(ServerSpeaker { server: server.clone(), spec, gpu_lock: Arc::new(Mutex::new(())) });
        let voice = ResolvedVoice { reference: PathBuf::from(var("VOXCPM_REF")), transcript: String::new(), style: "calm, warm".into(), cfg: 2.0, steps: 10 };
        let tts = VoxCpmTts::new(speaker, voice, Language::Km, 1016);
        let text = KM.repeat(repetitions);
        let expected = expected_ms(&text, Language::Km, 0);
        let started = std::time::Instant::now();
        let (pcm, words) = tts.synth(&text, &CancellationToken::new()).await.unwrap();
        let secs = started.elapsed().as_secs_f64();
        std::fs::write(out.join("e2e_khmer.wav"), wav::to_wav(&pcm)).unwrap();
        println!("{} ms audio (expected {expected} ms) in {secs:.1} s; {} sentence chunks", wav::ms(&pcm), words.len());
        server.stop().await;
        assert_eq!(words.len(), repetitions * 2);
        assert!(!suspicious_length(wav::ms(&pcm), expected));
    }
}
