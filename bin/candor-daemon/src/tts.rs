/// Text-to-Speech module for Candor AI.
///
/// Uses only open-source backends:
/// - **piper-tts** — neural TTS with natural voices (preferred)
/// - **espeak-ng** — lightweight formant synthesis (fallback)
///
/// Set `CANDOR_TTS_MODEL` to override the piper model path.
/// Set `CANDOR_TTS_VOICE` to override the espeak-ng voice (default: "en-us").
/// Set `CANDOR_AUDIO_OUTPUT` to override the playback device (default: "default").
use std::path::PathBuf;
use std::process::Stdio;

/// Supported TTS backends, probed at runtime.
#[derive(Debug, Clone, PartialEq)]
enum TtsBackend {
    /// Piper neural TTS (piper-tts) — best quality.
    Piper(PathBuf),
    /// espeak-ng — universally available, robotic but reliable.
    EspeakNg(PathBuf),
    /// No supported backend found.
    Unavailable,
}

impl TtsBackend {
    /// Probe the system for an installed TTS binary.
    fn probe() -> Self {
        // Check piper first (preferred), then espeak-ng.
        for name in &["piper", "espeak-ng", "espeak"] {
            if let Some(path) = find_on_path(name) {
                return match *name {
                    "piper" => Self::Piper(path),
                    "espeak-ng" | "espeak" => Self::EspeakNg(path),
                    _ => Self::EspeakNg(path),
                };
            }
        }
        Self::Unavailable
    }

    /// Human-readable label for the active backend.
    fn label(&self) -> &str {
        match self {
            Self::Piper(_) => "piper-tts",
            Self::EspeakNg(_) => "espeak-ng",
            Self::Unavailable => "unavailable",
        }
    }

    /// Speak the given text through the system audio output.
    async fn speak(&self, text: &str) -> Result<(), TtsError> {
        match self {
            Self::Piper(path) => {
                // piper-tts pipeline:
                //   echo "text" | piper --model <model> --output-raw | aplay -r 22050 -f S16_LE -c 1 -
                //
                // Model path resolution (in order of priority):
                //   1. CANDOR_TTS_MODEL env var
                //   2. ~/.local/share/piper/voices/en_US-lessac-medium.onnx
                //   3. /usr/share/piper/voices/en_US-lessac-medium.onnx
                let model = Self::resolve_piper_model();
                let model_ref = match &model {
                    Some(m) => m.as_str(),
                    None => "en_US-lessac-medium",
                };

                let device = std::env::var("CANDOR_AUDIO_OUTPUT")
                    .unwrap_or_else(|_| "default".into());

                // Stage 1: Piper generates raw PCM audio.
                let mut piper = tokio::process::Command::new(path)
                    .arg("--model")
                    .arg(model_ref)
                    .arg("--output-raw")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| TtsError::Backend(format!("piper spawn failed: {e}")))?;

                // Write text to piper's stdin.
                use tokio::io::AsyncWriteExt;
                if let Some(mut stdin) = piper.stdin.take() {
                    stdin.write_all(text.as_bytes()).await.ok();
                    // Close stdin so piper knows to stop reading.
                    drop(stdin);
                }

                // Read piper's stdout (raw PCM).
                let pcm_data = piper
                    .wait_with_output()
                    .await
                    .map_err(|e| TtsError::Backend(format!("piper execution failed: {e}")))?;

                if !pcm_data.status.success() {
                    return Err(TtsError::Backend(format!(
                        "piper exited with code {:?}",
                        pcm_data.status.code(),
                    )));
                }

                if pcm_data.stdout.is_empty() {
                    return Err(TtsError::Backend(
                        "piper produced no audio output".into(),
                    ));
                }

                // Stage 2: Pipe raw PCM through aplay.
                let mut aplay = tokio::process::Command::new("aplay")
                    .arg("-D")
                    .arg(&device)
                    .arg("-r")
                    .arg("22050")
                    .arg("-f")
                    .arg("S16_LE")
                    .arg("-c")
                    .arg("1")
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| TtsError::Playback(format!("aplay not found: {e}")))?;

                if let Some(mut stdin) = aplay.stdin.take() {
                    stdin
                        .write_all(&pcm_data.stdout)
                        .await
                        .map_err(|e| TtsError::Playback(format!("pipe to aplay failed: {e}")))?;
                    drop(stdin);
                }

                let play_status = aplay
                    .wait()
                    .await
                    .map_err(|e| TtsError::Playback(format!("aplay wait failed: {e}")))?;

                if !play_status.success() {
                    return Err(TtsError::Playback(format!(
                        "aplay exited with code {:?}",
                        play_status.code(),
                    )));
                }

                Ok(())
            }
            Self::EspeakNg(path) => {
                // espeak-ng pipeline:
                //   espeak-ng "text" --stdout | aplay
                let voice = std::env::var("CANDOR_TTS_VOICE").unwrap_or_else(|_| "en-us".into());
                let device = std::env::var("CANDOR_AUDIO_OUTPUT")
                    .unwrap_or_else(|_| "default".into());

                // espeak-ng generates WAV on stdout.
                let output = tokio::process::Command::new(path)
                    .arg(text)
                    .arg("-v")
                    .arg(&voice)
                    .arg("--stdout")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .await
                    .map_err(|e| TtsError::Backend(format!("espeak-ng execution failed: {e}")))?;

                if !output.status.success() {
                    return Err(TtsError::Backend(format!(
                        "espeak-ng exited with code {:?}",
                        output.status.code(),
                    )));
                }

                if output.stdout.is_empty() {
                    return Err(TtsError::Backend(
                        "espeak-ng produced no audio output".into(),
                    ));
                }

                // Play through aplay.
                let mut aplay = tokio::process::Command::new("aplay")
                    .arg("-D")
                    .arg(&device)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .map_err(|e| TtsError::Playback(format!("aplay not found: {e}")))?;

                use tokio::io::AsyncWriteExt;
                if let Some(mut stdin) = aplay.stdin.take() {
                    stdin
                        .write_all(&output.stdout)
                        .await
                        .map_err(|e| TtsError::Playback(format!("pipe to aplay failed: {e}")))?;
                    drop(stdin);
                }

                let play_status = aplay
                    .wait()
                    .await
                    .map_err(|e| TtsError::Playback(format!("aplay wait failed: {e}")))?;

                if !play_status.success() {
                    return Err(TtsError::Playback(format!(
                        "aplay exited with code {:?}",
                        play_status.code(),
                    )));
                }

                Ok(())
            }
            Self::Unavailable => Err(TtsError::Unavailable),
        }
    }

    /// Resolve the piper-tts model path.
    fn resolve_piper_model() -> Option<String> {
        // Check env var first.
        if let Ok(path) = std::env::var("CANDOR_TTS_MODEL") {
            if std::path::Path::new(&path).exists() {
                return Some(path);
            }
        }

        // Check common installation paths.
        let candidates = [
            dirs_or_defaults(),
        ];

        for base in &candidates {
            for variant in &[
                "en_US-lessac-medium.onnx",
                "en_US-amy-medium.onnx",
                "en_GB-southern_english_female-medium.onnx",
                "voices/en_US-lessac-medium.onnx",
            ] {
                let p = std::path::Path::new(base).join(variant);
                if p.exists() {
                    return Some(p.to_string_lossy().to_string());
                }
            }
        }

        None
    }
}

/// Get standard directories for piper model lookup.
fn dirs_or_defaults() -> String {
    if let Some(home) = dirs_next_or_fallback() {
        format!("{home}/.local/share/piper")
    } else {
        "/usr/share/piper".to_string()
    }
}

fn dirs_next_or_fallback() -> Option<String> {
    std::env::var("HOME").ok()
}

/// Errors from the TTS pipeline.
#[derive(Debug)]
pub enum TtsError {
    /// No TTS backend is installed.
    Unavailable,
    /// Backend-specific failure.
    Backend(String),
    /// Audio playback failure.
    Playback(String),
    /// I/O error.
    Io(std::io::Error),
}

impl std::fmt::Display for TtsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => write!(
                f,
                "No TTS backend found. Install piper-tts (recommended) or espeak-ng (fallback)."
            ),
            Self::Backend(e) => write!(f, "TTS backend error: {e}"),
            Self::Playback(e) => write!(f, "Audio playback error: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for TtsError {}

impl From<std::io::Error> for TtsError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Lazy-probed TTS backend (cached after first check).
static BACKEND: std::sync::OnceLock<TtsBackend> = std::sync::OnceLock::new();

fn backend() -> &'static TtsBackend {
    BACKEND.get_or_init(TtsBackend::probe)
}

/// Check whether a TTS backend is available.
pub fn is_available() -> bool {
    *backend() != TtsBackend::Unavailable
}

/// Speak the given text through the system audio output.
///
/// If no TTS backend is available, prints a warning and returns an error
/// directing the user to install piper-tts or espeak-ng.
pub async fn speak(text: &str) -> Result<(), TtsError> {
    let b = backend();
    if *b == TtsBackend::Unavailable {
        return Err(TtsError::Unavailable);
    }
    println!("🔊 Speaking… (backend: {})", b.label());
    b.speak(text).await
}

/// Helper: find a binary on PATH (shared with stt.rs).
fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").as_ref().and_then(|paths| {
        std::env::split_paths(paths).find_map(|dir| {
            let full = dir.join(name);
            if full.is_file() {
                Some(full)
            } else {
                let with_ext = dir.join(format!("{}.exe", name));
                if with_ext.is_file() {
                    Some(with_ext)
                } else {
                    None
                }
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_doesnt_panic() {
        let _ = TtsBackend::probe();
    }

    #[test]
    fn test_is_available_doesnt_panic() {
        let _ = is_available();
    }
}
