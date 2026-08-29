# Pi token-policy extension

This adapter targets Pi `0.84.4` and uses only the public extension surface.

## Install

Copy this `token-policy` directory to `<project-root>/.pi/extensions/token-policy/`.
Copy `configs/token-efficiency.example.yaml` to `<project-root>/.pi/token-policy.yaml` when configuration is needed. The default with no config is `observe`.

Ensure the target repository ignores:

```gitignore
.token-policy/
```

If `artifacts.root` is changed, add that repository-relative directory to the target's ignore rules too. Runtime artifacts, the ledger, and telemetry are local-only and use private permissions.

## Modes

- `observe`: records content-free tool/model metrics and does not modify tool results.
- `safe`: applies only allowlisted deterministic reducers to large Bash results, stores the exact original, and exposes exact retrieval on demand.
- `experimental`: currently behaves as observe; no quality-sensitive experimental policy is enabled.

Run observe first, record the same-task baseline, then change only `mode: safe` and rerun it. The extension never changes Pi's reasoning level, model selection, prompt history, provider adapters, or native compaction.

## Safe output flow

1. Pi runs the native tool and emits `tool_result`.
2. For a recognized test/compiler/lint/Git/package-manager command above the configured threshold, the extension stores the complete native Bash output when Pi provides `details.fullOutputPath`; otherwise it stores the received text byte-for-byte under `.token-policy/`.
3. A bounded deterministic excerpt is returned with an opaque `token-artifact:<id>` reference.
4. Pi dynamically activates the `token_context` retrieval tool only after the first reduction. Before that, it adds no permanent retrieval schema.
5. Retrieval supports exact byte ranges, lexical line search, and metadata. Pi's native `read` tool remains a fallback through the repository-relative artifact path.

Unknown commands, unsupported content, small results, reducer failures, metadata failures, and artifact-storage failures return the original result unchanged. If Pi's native full-output file cannot be read, reduction is skipped. The original `isError`, relevant details, cancellation, and native execution semantics are not replaced.

## Ledger and compaction

The concise ledger is written to `.token-policy/ledger.json` and checkpointed as a non-context Pi custom entry immediately before native compaction. It is restored from the latest branch checkpoint or local file on session start. It is not injected every turn and does not replace Pi's native summary or context rebuild.

## Verification

From the kit root:

```bash
bun test ./skeleton/pi/.pi/extensions/token-policy/tests
tsc --noEmit --target ES2022 --module NodeNext --moduleResolution NodeNext --allowImportingTsExtensions --skipLibCheck <extension-sources>
python scripts/public_repo_audit.py .
```

The observe fixture baseline used a synthetic 50,021-byte failing test output: raw and model-visible output were both 50,021 bytes. The safe fixture reduced the model-visible excerpt while verifying byte-for-byte recovery and preserved error status. These are harness-level measurements, not provider billing measurements.

## Rollback

Set `mode: observe` or remove the extension directory. Pi continues using its native tool output, provider behavior, caching, and compaction. Existing `.token-policy/` state can be deleted separately after diagnosis.
