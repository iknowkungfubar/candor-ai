/// Benchmarks for candor-core performance-critical paths.
///
/// These benchmarks serve as a performance regression detection suite.
/// They run via `cargo bench --package candor-core`.
use criterion::{Criterion, black_box, criterion_group, criterion_main};

use candor_core::ideal::{AcceptanceCriterion, IdealStateArtifact, VerificationMethod};
use candor_core::state::AgentState;

// ── AgentState benchmarks ──

fn bench_state_append_messages(c: &mut Criterion) {
    c.bench_function("state/append_100_messages", |b| {
        b.iter(|| {
            let mut state = AgentState::default();
            for i in 0..100 {
                state.append_message(black_box(&format!("message number {i} of the test")));
            }
            black_box(state.message_history.len());
        })
    });
}

fn bench_state_compaction(c: &mut Criterion) {
    c.bench_function("state/compaction_200chars", |b| {
        b.iter_batched(
            || {
                let mut state = AgentState::default();
                for i in 0..500 {
                    state.append_message(&format!(
                        "A reasonably long message that simulates real agent output \
                         with details about iteration {i}"
                    ));
                }
                state
            },
            |mut state| {
                state.compact_context(black_box(200));
                black_box(state.message_history.len());
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

fn bench_state_token_limit_check(c: &mut Criterion) {
    c.bench_function("state/is_over_token_limit", |b| {
        let mut state = AgentState::default();
        for i in 0..50 {
            state.append_message(&format!(
                "Message content for token check benchmark iteration {i}"
            ));
        }
        b.iter(|| {
            let over = state.is_over_token_limit();
            black_box(over);
        })
    });
}

// ── Ideal State Artifact benchmarks ──

fn bench_isa_creation(c: &mut Criterion) {
    c.bench_function("isa/create_small", |b| {
        b.iter(|| {
            let isa = IdealStateArtifact {
                id: black_box("bench-test".into()),
                goal: black_box("run a quick benchmark".into()),
                acceptance_criteria: vec![AcceptanceCriterion {
                    id: "c1".into(),
                    description: "first criterion".into(),
                    verification_method: VerificationMethod::ShellCommand {
                        command: "ls".into(),
                    },
                }],
                constraints: vec![],
                expected_artifacts: vec![],
                phase_requirements: Default::default(),
                fully_autonomous: true,
            };
            black_box(isa.id.len())
        })
    });
}

fn bench_isa_validate_criteria(c: &mut Criterion) {
    c.bench_function("isa/validate_10_criteria", |b| {
        let isa = IdealStateArtifact {
            id: "bench".into(),
            goal: "test".into(),
            acceptance_criteria: (0..10)
                .map(|i| AcceptanceCriterion {
                    id: format!("c{i}"),
                    description: format!("criterion {i}"),
                    verification_method: VerificationMethod::ShellCommand {
                        command: format!("echo {i}"),
                    },
                })
                .collect(),
            constraints: vec![],
            expected_artifacts: vec![],
            phase_requirements: Default::default(),
            fully_autonomous: true,
        };
        let active_task = "run a test benchmark".to_string();

        b.iter(|| {
            let result = (|| -> Result<(), String> {
                if isa.acceptance_criteria.is_empty() {
                    return Err("no criteria".into());
                }
                for criterion in &isa.acceptance_criteria {
                    let is_valid = match &criterion.verification_method {
                        VerificationMethod::ShellCommand { command }
                        | VerificationMethod::LintCheck { command } => !command.trim().is_empty(),
                        VerificationMethod::TestCase { test_name } => !test_name.trim().is_empty(),
                        VerificationMethod::FileExists { path } => !path.trim().is_empty(),
                        VerificationMethod::FileMatches { path, pattern } => {
                            !path.trim().is_empty() && !pattern.trim().is_empty()
                        }
                        VerificationMethod::HumanConfirmation { prompt } => {
                            !prompt.trim().is_empty()
                        }
                    };
                    if !is_valid {
                        return Err(format!("invalid: {}", criterion.id));
                    }
                }
                let task_lower = active_task.to_lowercase();
                let goal_lower = isa.goal.to_lowercase();
                if !task_lower.contains(&goal_lower) && !goal_lower.contains(&task_lower) {
                    return Err("alignment fail".into());
                }
                Ok(())
            })();
            black_box(result.is_ok());
        })
    });
}

// ── CoreError construction benchmarks ──

fn bench_error_construction(c: &mut Criterion) {
    use candor_core::error::CoreError;

    c.bench_function("error/construct_variants", |b| {
        b.iter(|| {
            let errors: Vec<CoreError> = vec![
                CoreError::Internal(black_box("test error".into())),
                CoreError::Io(black_box("file not found".into())),
                CoreError::Config(black_box("missing key".into())),
                CoreError::Serialization(black_box("json parse error".into())),
                CoreError::MaxIterationsReached,
                CoreError::HumanApprovalDenied,
            ];
            black_box(errors.len())
        })
    });
}

criterion_group!(
    name = state;
    config = Criterion::default().sample_size(100);
    targets =
        bench_state_append_messages,
        bench_state_compaction,
        bench_state_token_limit_check,
        bench_isa_creation,
        bench_isa_validate_criteria,
        bench_error_construction,
);
criterion_main!(state);
