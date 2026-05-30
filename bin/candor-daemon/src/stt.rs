/// Speech-to-Text module for Candor AI.
///
/// Records audio from the microphone and transcribes it via whisper-cpp
/// (subprocess) or another available STT backend. Falls back gracefully
/// when no backend is installed.
///
/// CLI usage:
///   candor --voice-task           # record & transcribe, then run as task
///   candor --voice-task "prompt"  # pre-seeded task prefix

use std::path::PathBuf;
use std::process::Stdio;

/// Supported STT backends, probed at runtime.
#[derive(Debug, Clone, PartialEq)]
enum WhisperBackend {
    /// `whisper-cpp` CLI is available on PATH.
    WhisperCpp(PathBuf),
    /// `whisper-cli` (OpenAI's whisper.cpp Python wrapper or similar).
    WhisperCli(PathBuf),
    /// No supported backend found.
    Unavailable,
}

impl WhisperBackend {
    /// Probe the system for an installed whisper binary.
    fn probe() -> Self {
        // Check common binary names in order of preference.
        for name in &["whisper-cpp", "whisper-cli", "whisper"] {
            if let Some(path) = find_on_path(name) {
                return match *name {
                    "whisper-cpp" => Self::WhisperCpp(path),
                    "whisper-cli" => Self::WhisperCli(path),
                    _ => Self::WhisperCli(path),
                };
            }
        }
        Self::Unavailable
    }

    /// Human-readable label for the active backend.
    fn label(&self) -> &str {
        match self {
            Self::WhisperCpp(_) => "whisper-cpp",
            Self::WhisperCli(_) => "whisper-cli",
            Self::Unavailable => "unavailable",
        }
    }

    /// Transcribe a WAV file, returning the transcribed text.
    async fn transcribe(&self, wav_path: &std::path::Path) -> Result<String, SttError> {
        match self {
            Self::WhisperCpp(path) => {
                // whisper-cpp CLI: whisper-cpp -f file.wav -nt
                // The -nt flag suppresses timestamps.
                let output = tokio::process::Command::new(path)
                    .arg("-f")
                    .arg(wav_path)
                    .arg("-nt")
                    .arg("--no-prints")
                    .arg("true")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .await
                    .map_err(|e| SttError::Backend(format!("whisper-cpp execution failed: {e}")))?;

                if !output.status.success() {
                    return Err(SttError::Backend(format!(
                        "whisper-cpp exited with code {:?}",
                        output.status.code(),
                    )));
                }

                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() {
                    return Err(SttError::NoSpeech);
                }
                Ok(text)
            }
            Self::WhisperCli(path) => {
                // Generic whisper CLI fallback: whisper file.wav --output_format txt
                let output = tokio::process::Command::new(path)
                    .arg(wav_path)
                    .arg("--output_format")
                    .arg("txt")
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .await
                    .map_err(|e| SttError::Backend(format!("whisper-cli execution failed: {e}")))?;

                if !output.status.success() {
                    return Err(SttError::Backend(format!(
                        "whisper-cli exited with code {:?}",
                        output.status.code(),
                    )));
                }

                let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if text.is_empty() {
                    return Err(SttError::NoSpeech);
                }
                Ok(text)
            }
            Self::Unavailable => Err(SttError::Unavailable),
        }
    }
}

/// Errors from the STT pipeline.
#[derive(Debug)]
pub enum SttError {
    /// No STT backend is installed on the system.
    Unavailable,
    /// Error accessing the microphone.
    Mic(String),
    /// Backend-specific failure.
    Backend(String),
    /// No speech detected in the recording.
    NoSpeech,
    /// I/O error.
    Io(std::io::Error),
}

impl std::fmt::Display for SttError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => write!(
                f,
                "No STT backend found. Install whisper-cpp (https://github.com/ggerganov/whisper.cpp) or set CANDOR_WHISPER_MODEL."
            ),
            Self::Mic(e) => write!(f, "Microphone error: {e}"),
            Self::Backend(e) => write!(f, "STT backend error: {e}"),
            Self::NoSpeech => write!(f, "No speech detected"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for SttError {}

impl From<std::io::Error> for SttError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Lazy-probed STT backend (cached after first check).
static BACKEND: std::sync::OnceLock<WhisperBackend> = std::sync::OnceLock::new();

fn backend() -> &'static WhisperBackend {
    BACKEND.get_or_init(WhisperBackend::probe)
}

/// Record audio from the default microphone using `arecord`.
///
/// Defaults to: 16-bit PCM, 16 kHz mono, stored as WAV in a temp file.
/// Set `CANDOR_AUDIO_DEVICE` to override the ALSA device (e.g. "plughw:1,0").
/// Set `CANDOR_RECORD_SECONDS` to override the recording duration (default 5).
pub async fn record_audio() -> Result<PathBuf, SttError> {
    let device = std::env::var("CANDOR_AUDIO_DEVICE").unwrap_or_else(|_| "default".into());
    let duration = std::env::var("CANDOR_RECORD_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(5);

    let tmp_dir = std::env::temp_dir().join("candor-stt");
    tokio::fs::create_dir_all(&tmp_dir)
        .await
        .map_err(SttError::Io)?;

    let wav_path = tmp_dir.join(format!("voice_{}.wav", chrono::Utc::now().timestamp()));

    // arecord -D <device> -r 16000 -f S16_LE -c 1 -d <duration> <file>
    println!("🎙️  Recording for {duration}s (device: {device})…");
    let status = tokio::process::Command::new("arecord")
        .arg("-D")
        .arg(&device)
        .arg("-r")
        .arg("16000")
        .arg("-f")
        .arg("S16_LE")
        .arg("-c")
        .arg("1")
        .arg("-d")
        .arg(duration.to_string())
        .arg(&wav_path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map_err(|e| SttError::Mic(format!("arecord not found: {e}")))?;

    if !status.success() {
        return Err(SttError::Mic(format!(
            "arecord failed (exit code {:?})",
            status.code()
        )));
    }

    if !wav_path.exists() {
        return Err(SttError::Mic("No audio file was created".into()));
    }

    println!("✅ Recording saved to {}", wav_path.display());
    Ok(wav_path)
}

/// Record and transcribe audio from the microphone.
///
/// Returns the transcribed text. If no backend is available, returns an
/// error prompting the user to install whisper-cpp.
pub async fn transcribe_mic() -> Result<String, SttError> {
    let backend = backend();
    if *backend == WhisperBackend::Unavailable {
        return Err(SttError::Unavailable);
    }

    let wav_path = record_audio().await?;
    println!("🔊 Transcribing… (backend: {})", backend.label());
    let text = backend.transcribe(&wav_path).await?;

    // Clean up the temp file.
    let _ = tokio::fs::remove_file(&wav_path).await;

    println!("📝 Transcription: \"{text}\"");
    Ok(text)
}

/// Helper: find a binary on PATH.
fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .as_ref()
        .and_then(|paths| {
            std::env::split_paths(paths).find_map(|dir| {
                let full = dir.join(name);
                if full.is_file() {
                    // Also try with .exe on Windows (harmless on Unix).
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
        // Just ensure probing doesn't crash.
        let _ = WhisperBackend::probe();
    }

    #[test]
    fn test_find_on_path_known() {
        // sh should always exist.
        assert!(find_on_path("sh").is_some() || cfg!(target_os = "windows"));
    }

    #[test]
    fn test_find_on_path_nonexistent() {
        assert!(find_on_path("this-binary-should-not-exist-42").is_none());
    }
}
