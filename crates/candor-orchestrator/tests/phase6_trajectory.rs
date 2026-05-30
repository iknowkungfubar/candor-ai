/// Phase 6 integration tests: trajectory extraction + LoRA fine-tuning.
use std::path::PathBuf;

use candor_orchestrator::skills::{Skill, extract_skills_from_log, persist_skills};
use candor_orchestrator::trajectory::{
    TrajectoryEntry, append_to_jsonl, LoRAPipeline,
};

// ── Criterion 1: Learn node generates .md skill file ──

#[test]
fn test_skill_to_markdown_format() {
    let skill = Skill {
        name: "test-skill".into(),
        description: "Test skill for Phase 6".into(),
        trigger: "when testing".into(),
        steps: vec!["Step 1: Observe".into(), "Step 2: Build".into(), "Step 3: Verify".into()],
        tools_used: vec!["read_file".into(), "shell".into(), "run_tests".into()],
        pitfalls: vec!["Don't forget to lock the mutex".into()],
        use_count: 1,
    };

    let md = skill.to_markdown();
    assert!(md.contains("name: test-skill"));
    assert!(md.contains("Step 1: Observe"));
    assert!(md.contains("Step 2: Build"));
    assert!(md.contains("Step 3: Verify"));
    assert!(md.contains("Pitfalls"));
    assert!(md.contains("Don't forget to lock the mutex"));
    assert!(md.contains("use_count: 1"));
}

#[test]
fn test_extract_skills_from_successful_trajectory() {
    let log = vec![
        "Task: build auth module".into(),
        "Observe: project structure scanned".into(),
        "Plan: implementation plan generated".into(),
        "Build: 3 files written".into(),
        "Execute: cargo check complete".into(),
        "Verify: PASSED".into(),
        "Task complete".into(),
    ];

    let skills = extract_skills_from_log(
        "build auth module",
        &log,
        &["read_file".into(), "write_file".into(), "shell".into()],
    );

    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "build-auth-module");
    assert_eq!(skills[0].tools_used.len(), 3);
}

#[test]
fn test_no_skill_from_failed_trajectory() {
    let log = vec![
        "Task: broken feature".into(),
        "Build: 0 files written".into(),
        "Verify: FAILED".into(),
    ];

    let skills = extract_skills_from_log("broken feature", &log, &[]);
    assert_eq!(skills.len(), 0);
}

#[test]
fn test_extract_skills_with_pitfalls() {
    let log = vec![
        "Task: fix race condition".into(),
        "Execute: cargo check failed — missing import".into(),
        "Execute: cargo check complete after fix".into(),
        "Verify: PASSED".into(),
        "Task complete".into(),
    ];

    let skills = extract_skills_from_log("fix race condition", &log, &["shell".into()]);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].pitfalls.len(), 1);
    // Pitfall should contain the error
    assert!(skills[0].pitfalls[0].contains("failed"));
}

#[tokio::test]
async fn test_persist_skills_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let skills_dir = dir.path().join("skills");

    let skills = vec![Skill {
        name: "test-skill".into(),
        description: "A test skill".into(),
        trigger: "testing".into(),
        steps: vec!["Step 1".into()],
        tools_used: vec!["shell".into()],
        pitfalls: vec![],
        use_count: 1,
    }];

    let written = persist_skills(&skills, &skills_dir).await.unwrap();
    assert_eq!(written, 1);

    let skill_path = skills_dir.join("test-skill.SKILL.md");
    assert!(skill_path.exists());

    let content = tokio::fs::read_to_string(&skill_path).await.unwrap();
    assert!(content.contains("name: test-skill"));
}

// ── Criterion 2: Daily execution logs → JSONL ──

#[tokio::test]
async fn test_append_trajectory_to_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daily.jsonl");

    let entry = TrajectoryEntry::new("session-42", "build", "cargo build", "success");
    append_to_jsonl(&entry, &path).await.unwrap();

    assert!(path.exists());

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    assert!(content.contains("session-42"));
    assert!(content.contains("build"));
}

#[tokio::test]
async fn test_jsonl_appends_multiple_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("daily.jsonl");

    for phase in &["observe", "think", "plan", "build", "execute", "verify", "learn"] {
        let entry = TrajectoryEntry::new("sess-1", phase, &format!("did {phase}"), "ok");
        append_to_jsonl(&entry, &path).await.unwrap();
    }

    let content = tokio::fs::read_to_string(&path).await.unwrap();
    assert_eq!(content.lines().count(), 7);
    assert!(content.contains("observe"));
    assert!(content.contains("learn"));
}

#[tokio::test]
async fn test_jsonl_nonexistent_directory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("subdir").join("trajectories.jsonl");

    let entry = TrajectoryEntry::new("sess-1", "verify", "tests", "passed");
    append_to_jsonl(&entry, &path).await.unwrap();

    assert!(path.exists());
}

// ── Criterion 3: LoRA pipeline provisioning ──

#[test]
fn test_lora_pipeline_validate_missing_input() {
    let pipeline = LoRAPipeline::new(
        PathBuf::from("/nonexistent/traj.jsonl"),
        PathBuf::from("/tmp/test-lora"),
        "qwen3-1.5b".into(),
    );
    assert!(!pipeline.validate().unwrap());
}

#[tokio::test]
async fn test_lora_pipeline_provision_writes_config() {
    let dir = tempfile::tempdir().unwrap();
    let jsonl = dir.path().join("trajectories.jsonl");
    tokio::fs::write(&jsonl, "{}").await.unwrap();

    let pipeline = LoRAPipeline::new(
        jsonl,
        dir.path().join("lora_weights"),
        "phi-3-mini".into(),
    );

    assert!(pipeline.validate().unwrap());
    pipeline.provision().await.unwrap();

    let config_path = dir.path().join("lora_weights").join("lora_pipeline_config.json");
    assert!(config_path.exists());

    let config = tokio::fs::read_to_string(&config_path).await.unwrap();
    assert!(config.contains("lora_fine_tuning"));
    assert!(config.contains("phi-3-mini"));
    assert!(config.contains("rank"));
    assert!(config.contains("alpha"));
}

#[tokio::test]
async fn test_lora_pipeline_creates_output_dir() {
    let dir = tempfile::tempdir().unwrap();
    let jsonl = dir.path().join("trajectories.jsonl");
    tokio::fs::write(&jsonl, "{}").await.unwrap();

    let output = dir.path().join("auto_created_lora_dir");
    let pipeline = LoRAPipeline::new(jsonl, output.clone(), "model".into());

    assert!(pipeline.validate().unwrap());
    assert!(output.exists());
}
