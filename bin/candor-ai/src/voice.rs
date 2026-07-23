use std::sync::Arc;

use candor_cognitive::LlmRequest;
use candor_memory::store::MemorySystem;
use candor_orchestrator::OrchestratorEngine;

use crate::backend;
use crate::display::{CYAN, GREEN, YELLOW, RED, BOLD, RESET};
use crate::stt;
use crate::tts;

/// Execute a one-shot voice task: transcribe, then run as a CLI task.
pub async fn run_voice_task(
    prompt: Option<String>,
    _duration: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = stt::transcribe_mic()
        .await
        .map_err(|e| Box::new(std::io::Error::other(format!("Voice error: {e}"))))?;
    let task = if let Some(ref p) = prompt {
        format!("{p} {text}")
    } else {
        text
    };
    println!("\n  {CYAN}You said:{RESET} {}\n", task);
    let cognitive = backend::build_cognitive(None, None, None, None).await?;
    let memory = Arc::new(MemorySystem::new(384).await?);
    let orch = Arc::new(tokio::sync::Mutex::new(
        OrchestratorEngine::new(cognitive, memory, 100).await?,
    ));
    crate::run_cli_task(task, orch).await
}

/// Interactive voice conversation loop.
///
/// For each turn:
///   1. Record audio from microphone (STT)
///   2. Transcribe with whisper-cpp
///   3. Process as a chat message via the cognitive engine
///   4. Speak the response aloud (TTS)
///   5. Loop until the user says "exit", "quit", or max_turns reached
pub async fn run_voice_interactive(
    initial_prompt: Option<String>,
    duration: u64,
    max_turns: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let cognitive = backend::build_cognitive(None, None, None, None).await?;

    let tts_ok = tts::is_available();
    if !tts_ok {
        println!("  {YELLOW}⚠ TTS backend not found. Install piper-tts or espeak-ng for voice responses.{RESET}");
    }

    println!("  {GREEN}Say '{CYAN}exit{GREEN}' or '{CYAN}quit{GREEN}' to stop.{RESET}");
    println!("  {GREEN}Max {max_turns} turns.{RESET}\n");

    let exit_words = ["exit", "quit", "goodbye", "stop", "done"];

    for turn in 1..=max_turns {
        println!("\n  {BOLD}[Turn {turn}/{max_turns}]{RESET}");

        // -- Step 1: Listen --
        let text = match stt::transcribe_mic_with_duration(duration).await {
            Ok(t) => t,
            Err(stt::SttError::NoSpeech) => {
                println!("  {YELLOW}No speech detected listening again...{RESET}");
                continue;
            }
            Err(e) => {
                eprintln!("  {RED}STT error: {e}{RESET}");
                println!("  {YELLOW}Type your message instead (or 'exit' to quit):{RESET}");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                let input = input.trim().to_string();
                if input.is_empty() {
                    continue;
                }
                input
            }
        };

        let task_text = if let Some(ref p) = initial_prompt {
            format!("{p} {text}")
        } else {
            text.clone()
        };

        println!("  {CYAN}You:{RESET} {text}");

        if exit_words.contains(&task_text.to_lowercase().as_str()) {
            println!("  {GREEN}Goodbye!{RESET}");
            if tts_ok {
                let _ = tts::speak("Goodbye!").await;
            }
            break;
        }

        // -- Step 2: Think --
        println!("  {YELLOW}Thinking...{RESET}");
        let request = LlmRequest {
            system_prompt: Some(
                "You are a helpful voice assistant. Keep responses concise and conversational \
                 suitable for being read aloud. Answer in 1-3 sentences when possible."
                    .into(),
            ),
            prompt: task_text,
            max_tokens: Some(256),
            temperature: Some(0.7),
            stream: false,
            model_override: None,
        };

        let response = match cognitive.generate(&request).await {
            Ok(r) => r,
            Err(e) => {
                eprintln!("  {RED}LLM error: {e}{RESET}");
                println!("  {YELLOW}Sorry, I couldn't process that.{RESET}");
                continue;
            }
        };

        println!("  {GREEN}Candor:{RESET} {response}");

        // -- Step 3: Speak --
        if tts_ok {
            match tts::speak(&response).await {
                Ok(()) => {}
                Err(tts::TtsError::Unavailable) => {}
                Err(e) => {
                    eprintln!("  {YELLOW}TTS warning: {e}{RESET}");
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }

    println!("\n  {BOLD}Voice session ended.{RESET}");
    Ok(())
}