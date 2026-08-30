import { randomUUID } from "node:crypto";
import { lstat, readFile } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";
import { tmpdir } from "node:os";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { ArtifactStore, type ArtifactRecord } from "./artifacts.ts";
import { loadConfig, resolveRuntimeRoot, type PolicyConfig } from "./config.ts";
import { addModifiedFile, createLedger, ledgerFromBranch, loadLedger, LEDGER_ENTRY_TYPE, saveLedger, updateLedger, type ExecutionLedger } from "./ledger.ts";
import { classifyCommand, type OutputClass } from "./reducers/classify.ts";
import { reduceOutput } from "./reducers/index.ts";
import { retrieveArtifact, type RetrievalRequest } from "./retrieval.ts";
import { event, TelemetryStore } from "./telemetry.ts";

const TOKEN_CONTEXT_TOOL = "token_context";
const TOKEN_CONTEXT_PARAMETERS = {
	type: "object",
	additionalProperties: false,
	properties: {
		operation: { type: "string", enum: ["read", "search", "metadata"] },
		artifact: { type: "string", description: "Opaque token-artifact ID" },
		query: { type: "string" },
		offset: { type: "integer", minimum: 0 },
		limit: { type: "integer", minimum: 1, maximum: 50000 },
	},
	required: ["operation", "artifact"],
} as any;

interface Runtime {
	cwd: string;
	config: PolicyConfig;
	root: string;
	runId: string;
	turnId?: string;
	telemetry: TelemetryStore;
	artifacts?: ArtifactStore;
	ledger: ExecutionLedger;
	ledgerWrites: Promise<void>;
}

function contentBytes(content: readonly unknown[]): number {
	return content.reduce<number>((total, item) => {
		if (!item || typeof item !== "object") return total;
		const block = item as { type?: string; text?: string; data?: string };
		if (block.type === "text" && typeof block.text === "string") return total + Buffer.byteLength(block.text, "utf8");
		if (block.type === "image" && typeof block.data === "string") return total + Buffer.byteLength(block.data, "utf8");
		return total;
	}, 0);
}

function singleText(content: readonly unknown[]): string | undefined {
	if (content.length !== 1) return undefined;
	const block = content[0];
	if (!block || typeof block !== "object") return undefined;
	const text = (block as { type?: string; text?: unknown });
	return text.type === "text" && typeof text.text === "string" ? text.text : undefined;
}

function nativeFullOutputPath(toolEvent: unknown): string | null | undefined {
	if (!toolEvent || typeof toolEvent !== "object") return undefined;
	const event = toolEvent as { toolName?: unknown; details?: unknown };
	if (event.toolName !== "bash" || !event.details || typeof event.details !== "object") return undefined;
	const path = (event.details as { fullOutputPath?: unknown }).fullOutputPath;
	if (typeof path !== "string" || path.length === 0) return undefined;
	const relativeToTemp = relative(resolve(tmpdir()), resolve(path));
	if (isAbsolute(relativeToTemp) || relativeToTemp === ".." || relativeToTemp.startsWith("../") || relativeToTemp.startsWith("..\\")) return null;
	return path;
}

async function readNativeFullOutput(toolEvent: unknown, displayedText: string, signal?: AbortSignal): Promise<string | undefined> {
	const path = nativeFullOutputPath(toolEvent);
	if (path === undefined) return displayedText;
	if (path === null) return undefined;
	try {
		if (signal?.aborted) return undefined;
		const info = await lstat(path);
		if (info.isSymbolicLink() || !info.isFile()) return undefined;
		const full = await readFile(path, { encoding: "utf8", signal });
		return signal?.aborted ? undefined : full;
	} catch {
		return undefined;
	}
}

function safeRelativePath(cwd: string, value: unknown): string | null {
	if (typeof value !== "string" || !value) return null;
	const candidate = resolve(cwd, value);
	const result = relative(cwd, candidate).replaceAll("\\", "/");
	return result === "" ? "." : result === ".." || result.startsWith("../") ? null : result;
}

function modelUsageFields(message: unknown): Record<string, unknown> {
	if (!message || typeof message !== "object") return {};
	const value = (message as { usage?: unknown }).usage;
	if (!value || typeof value !== "object") return {};
	const usage = value as Record<string, unknown>;
	const numberOrNull = (field: string): number | null => (typeof usage[field] === "number" ? usage[field] as number : null);
	return {
		input_tokens: numberOrNull("input"),
		cached_input_tokens: numberOrNull("cacheRead"),
		cache_write_tokens: numberOrNull("cacheWrite"),
		reasoning_tokens: numberOrNull("reasoning"),
		output_tokens: numberOrNull("output"),
	};
}

function excerpt(text: string, head = 4, tail = 8): string {
	const lines = text.split(/\r?\n/);
	if (lines.length <= head + tail) return lines.join("\n").trim();
	return [...lines.slice(0, head), `[${lines.length - head - tail} lines omitted]`, ...lines.slice(-tail)].join("\n").trim();
}

function detailsWithArtifact(details: unknown, record: ArtifactRecord, reducer: string): unknown {
	const tokenPolicy = {
		artifact_id: record.metadata.id,
		reducer,
		raw_bytes: record.metadata.stdout_bytes,
		model_path: record.modelPath,
	};
	if (details && typeof details === "object" && !Array.isArray(details)) {
		const base = { ...(details as Record<string, unknown>) };
		// Pi's native fullOutputPath is absolute and is not needed after the exact copy.
		delete base.fullOutputPath;
		return { ...base, token_policy: tokenPolicy };
	}
	return { token_policy: tokenPolicy };
}

function eventFor(runtime: Runtime, eventType: Parameters<typeof event>[1], fields: Record<string, unknown>) {
	return event(runtime.runId, eventType, { turn_id: runtime.turnId ?? null, ...fields });
}

async function recordTelemetry(runtime: Runtime, eventType: Parameters<typeof event>[1], fields: Record<string, unknown>): Promise<void> {
	try {
		await runtime.telemetry.record(eventFor(runtime, eventType, fields));
	} catch {
		// Telemetry is diagnostic and must never affect the agent loop.
	}
}

async function recordPolicyError(runtime: Runtime, errorKind: string): Promise<void> {
	await recordTelemetry(runtime, "error", { error_kind: errorKind });
}

async function persistLedger(runtime: Runtime): Promise<void> {
	if (!runtime.config.ledger.enabled) return;
	const snapshot = runtime.ledger;
	runtime.ledgerWrites = runtime.ledgerWrites.then(async () => {
		try {
			await saveLedger(runtime.root, snapshot);
		} catch {
			await recordPolicyError(runtime, "ledger_storage");
		}
	});
	await runtime.ledgerWrites;
}

async function checkpointLedger(runtime: Runtime, pi: ExtensionAPI): Promise<void> {
	if (!runtime.config.ledger.enabled) return;
	await persistLedger(runtime);
	try {
		pi.appendEntry(LEDGER_ENTRY_TYPE, runtime.ledger);
	} catch {
		await recordPolicyError(runtime, "ledger_session_checkpoint");
	}
}

function activateRetrieval(pi: ExtensionAPI): void {
	try {
		const active = pi.getActiveTools();
		if (!active.includes(TOKEN_CONTEXT_TOOL)) pi.setActiveTools([...active, TOKEN_CONTEXT_TOOL]);
	} catch {
		// The relative artifact path remains usable with Pi's native read tool.
	}
}

async function transformSafeToolResult(
	runtime: Runtime,
	pi: ExtensionAPI,
	event: any,
	rawText: string,
	outputClass: OutputClass,
	rawBytes: number,
	signal?: AbortSignal,
): Promise<{ content: [{ type: "text"; text: string }]; details: unknown } | undefined> {
	const outputConfig = runtime.config.tool_output;
	const reduceEnabled = outputConfig.reducers[outputClass] === true;
	if (rawBytes <= outputConfig.passthrough_bytes) return undefined;
	const aboveReduceThreshold = rawBytes > outputConfig.structured_reduce_bytes;
	const aboveExternalizeThreshold = rawBytes > outputConfig.externalize_bytes;
	if ((!aboveReduceThreshold || !reduceEnabled) && !aboveExternalizeThreshold) return undefined;
	if (!runtime.artifacts || !outputConfig.preserve_raw) return undefined;

	let record: ArtifactRecord;
	try {
		record = await runtime.artifacts.save(rawText, {
			runId: runtime.runId,
			turnId: runtime.turnId,
			tool: event.toolName,
			commandClass: outputClass,
			repoRelativeCwd: ".",
			modelVisibleBytes: rawBytes,
		}, signal);
	} catch {
		await recordPolicyError(runtime, "artifact_storage");
		return undefined;
	}

	let visibleText: string;
	let reducerName: string;
	try {
		if (signal?.aborted) return undefined;
		if (aboveReduceThreshold && reduceEnabled) {
			const reduced = reduceOutput(outputClass, rawText, { isError: event.isError, artifactId: record.metadata.id });
			visibleText = [
				`[token-policy] Reduced ${outputClass} output from ${rawBytes} bytes.`,
				`status: ${event.isError ? "failed" : "completed"}`,
				`exact artifact: ${record.metadata.id}`,
				`Use ${TOKEN_CONTEXT_TOOL} with operation=read and this artifact, or read ${record.modelPath} in byte ranges.`,
				"",
				"Relevant output:",
				reduced.text,
			].join("\n");
			reducerName = reduced.name;
		} else {
			visibleText = [
				`[token-policy] Externalized ${outputClass} output (${rawBytes} bytes).`,
				`status: ${event.isError ? "failed" : "completed"}`,
				`exact artifact: ${record.metadata.id}`,
				`Use ${TOKEN_CONTEXT_TOOL} with operation=read and this artifact, or read ${record.modelPath} in byte ranges.`,
				"",
				excerpt(rawText),
			].join("\n");
			reducerName = "externalized";
		}
		await runtime.artifacts.updateMetadata(record.metadata.id, {
			reducer: reducerName,
			model_visible_bytes: Buffer.byteLength(visibleText, "utf8"),
		}, signal);
	} catch {
		await recordPolicyError(runtime, "reducer_or_metadata");
		return undefined;
	}

	activateRetrieval(pi);
	return {
		content: [{ type: "text", text: visibleText }],
		details: detailsWithArtifact(event.details, record, reducerName),
	};
}

export default function tokenPolicy(pi: ExtensionAPI): void {
	let runtime: Runtime | undefined;
	let initiallyActiveTools: string[] | undefined;
	try {
		initiallyActiveTools = pi.getActiveTools();
	} catch {
		// The active-tool list may not be bound during extension discovery.
	}

	pi.registerTool({
		name: TOKEN_CONTEXT_TOOL,
		label: "Token context",
		description: "Read exact text or search metadata for a token-artifact created by token-policy.",
		parameters: TOKEN_CONTEXT_PARAMETERS,
		async execute(_toolCallId, params, signal, _onUpdate, _ctx) {
			if (signal?.aborted) return { content: [{ type: "text", text: "Artifact retrieval cancelled" }], details: {} };
			if (!runtime || !runtime.artifacts) throw new Error("token-policy artifact retrieval is unavailable");
			if (!params || typeof params !== "object") throw new Error("invalid token_context parameters");
			const request = params as RetrievalRequest;
			if (!["read", "search", "metadata"].includes(request.operation)) throw new Error("unsupported retrieval operation");
			try {
				const text = await retrieveArtifact(runtime.artifacts, request, runtime.config.retrieval.max_initial_results, signal);
				return { content: [{ type: "text", text }], details: {} };
			} catch (error) {
				await recordPolicyError(runtime, "artifact_retrieval");
				if (signal?.aborted) return { content: [{ type: "text", text: "Artifact retrieval cancelled" }], details: {} };
				throw error;
			}
		},
	});

	pi.on("session_start", async (_event, ctx) => {
		const loaded = await loadConfig(ctx.cwd);
		const root = resolveRuntimeRoot(ctx.cwd, loaded.config.artifacts.root);
		const telemetry = new TelemetryStore(root, loaded.config.telemetry.enabled);
		runtime = {
			cwd: ctx.cwd,
			config: loaded.config,
			root,
			runId: `run-${randomUUID()}`,
			telemetry,
			ledger: createLedger(),
			ledgerWrites: Promise.resolve(),
		};
		if (loaded.config.mode === "safe" && loaded.config.artifacts.enabled && loaded.config.tool_output.preserve_raw) {
			runtime.artifacts = new ArtifactStore({
				root,
				modelRoot: loaded.config.artifacts.root,
				retentionDays: loaded.config.artifacts.retention_days,
				maxTotalMb: loaded.config.artifacts.max_total_mb,
			});
		}
		try {
			await telemetry.initialize();
		} catch {
			await recordPolicyError(runtime, "telemetry_storage");
		}
		if (runtime.artifacts) {
			try {
				await runtime.artifacts.initialize();
				await runtime.artifacts.cleanup();
			} catch {
				await recordPolicyError(runtime, "artifact_storage");
			}
		}
		if (loaded.config.ledger.enabled) {
			const manager = ctx.sessionManager as { getBranch?: () => readonly unknown[] } | undefined;
			const branchLedger = ledgerFromBranch(manager?.getBranch?.() ?? []);
			const fileLedger = await loadLedger(root);
			if (branchLedger && (!fileLedger || branchLedger.updated_at >= fileLedger.updated_at)) runtime.ledger = branchLedger;
			else if (fileLedger) runtime.ledger = fileLedger;
		}
		try {
			const active = pi.getActiveTools();
			const wasAlreadyActive = initiallyActiveTools?.includes(TOKEN_CONTEXT_TOOL) ?? false;
			if (!wasAlreadyActive && active.includes(TOKEN_CONTEXT_TOOL)) pi.setActiveTools(active.filter((name) => name !== TOKEN_CONTEXT_TOOL));
		} catch {
			// Tool activation is optional; the native read fallback remains available.
		}
	});

	pi.on("turn_start", async (_event, _ctx) => {
		if (runtime) runtime.turnId = `turn-${randomUUID()}`;
	});

	pi.on("agent_start", async () => {
		if (!runtime) return;
		runtime.ledger = updateLedger(runtime.ledger, { current_phase: "working", next_action: "continue current task" });
		await persistLedger(runtime);
	});

	pi.on("agent_settled", async () => {
		if (!runtime) return;
		runtime.ledger = updateLedger(runtime.ledger, { current_phase: "settled", next_action: "ready for next task" });
		await persistLedger(runtime);
		await recordTelemetry(runtime, "session_summary", { task_success: null });
	});

	pi.on("message_end", async (event, ctx) => {
		if (!runtime || event.message.role !== "assistant") return;
		await recordTelemetry(runtime, "model_call", {
			provider: event.message.provider ?? ctx.model?.provider ?? null,
			model: event.message.model ?? ctx.model?.id ?? null,
			...modelUsageFields(event.message),
			context_utilization_percent: ctx.getContextUsage()?.percent ?? null,
		});
	});

	pi.on("tool_call", async (event) => {
		if (!runtime || !runtime.config.ledger.enabled) return;
		if (event.toolName !== "edit" && event.toolName !== "write") return;
		const path = safeRelativePath(runtime.cwd, (event.input as { path?: unknown }).path);
		if (!path) return;
		runtime.ledger = addModifiedFile(runtime.ledger, path, "touched");
		await persistLedger(runtime);
	});

	pi.on("tool_result", async (event, ctx) => {
		if (!runtime) return;
		const displayedBytes = contentBytes(event.content);
		const command = event.toolName === "bash" ? (event.input as { command?: unknown }).command : undefined;
		const outputClass = typeof command === "string" ? classifyCommand(command) : undefined;
		const displayedText = singleText(event.content);
		let rawText = displayedText;
		let rawBytes = displayedBytes;
		if (!ctx.signal?.aborted && runtime.config.mode === "safe" && outputClass && displayedText !== undefined) {
			rawText = await readNativeFullOutput(event, displayedText, ctx.signal);
			if (rawText !== undefined) rawBytes = Buffer.byteLength(rawText, "utf8");
		}
		if (ctx.signal?.aborted || runtime.config.mode !== "safe" || !outputClass || rawText === undefined) {
			await recordTelemetry(runtime, "tool_result", {
				tool: event.toolName,
				command_class: outputClass ?? null,
				raw_tool_bytes: rawBytes,
				model_visible_tool_bytes: rawBytes,
				reduced: false,
			});
			return;
		}

		const transformed = await transformSafeToolResult(runtime, pi, event, rawText, outputClass, rawBytes, ctx.signal);
		if (!transformed) {
			await recordTelemetry(runtime, "tool_result", {
				tool: event.toolName,
				command_class: outputClass,
				raw_tool_bytes: rawBytes,
				model_visible_tool_bytes: rawBytes,
				reduced: false,
			});
			return;
		}

		const artifactDetails = transformed.details as { token_policy?: { artifact_id?: string; reducer?: string } };
		const artifactId = artifactDetails.token_policy?.artifact_id ?? null;
		const reducer = artifactDetails.token_policy?.reducer ?? null;
		await recordTelemetry(runtime, "artifact", {
			artifact_id: artifactId,
			raw_tool_bytes: rawBytes,
			model_visible_tool_bytes: Buffer.byteLength(transformed.content[0].text, "utf8"),
		});
		await recordTelemetry(runtime, "tool_result", {
			tool: event.toolName,
			command_class: outputClass,
			raw_tool_bytes: rawBytes,
			model_visible_tool_bytes: Buffer.byteLength(transformed.content[0].text, "utf8"),
			reduced: true,
			reducer,
			artifact_id: artifactId,
		});
		if (runtime.config.ledger.enabled) {
			runtime.ledger = updateLedger(runtime.ledger, {
				tests: { last_command: outputClass, status: event.isError ? "failed" : "passed", artifact: artifactId },
			});
			await persistLedger(runtime);
		}
		return transformed;
	});

	pi.on("session_before_compact", async (event) => {
		if (!runtime) return;
		runtime.ledger = updateLedger(runtime.ledger, { current_phase: "before-compaction", next_action: "resume from ledger after compaction" });
		await checkpointLedger(runtime, pi);
		await recordTelemetry(runtime, "compaction", { phase: "before", reason: event.reason });
	});

	pi.on("session_compact", async (event) => {
		if (!runtime) return;
		runtime.ledger = updateLedger(runtime.ledger, { current_phase: "resumed-after-compaction", next_action: "continue from verified ledger" });
		await persistLedger(runtime);
		await recordTelemetry(runtime, "compaction", { phase: "after", reason: event.reason, from_extension: event.fromExtension });
	});

	pi.on("session_compact_failed", async (event) => {
		if (!runtime) return;
		runtime.ledger = updateLedger(runtime.ledger, { current_phase: "compaction-failed", next_action: "inspect native compaction failure" });
		await persistLedger(runtime);
		await recordTelemetry(runtime, "compaction", { phase: "failed", reason: event.reason, aborted: event.aborted, will_retry: event.willRetry });
	});

	pi.registerCommand("token-policy", {
		description: "Show token-policy mode and local ledger status",
		handler: async (_args, ctx) => {
			if (!runtime) {
				ctx.ui.notify("token-policy has not received session_start", "warning");
				return;
			}
			ctx.ui.notify(`token-policy mode=${runtime.config.mode}; phase=${runtime.ledger.current_phase}; run=${runtime.runId}`, "info");
		},
	});
}
