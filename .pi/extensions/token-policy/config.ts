import { readFile } from "node:fs/promises";
import { isAbsolute, relative, resolve } from "node:path";

export type PolicyMode = "observe" | "safe" | "experimental";

export interface ToolOutputConfig {
	passthrough_bytes: number;
	structured_reduce_bytes: number;
	externalize_bytes: number;
	preserve_raw: boolean;
	fail_open: boolean;
	reducers: Record<string, boolean>;
}

export interface ArtifactConfig {
	enabled: boolean;
	root: string;
	retention_days: number;
	max_total_mb: number;
	hash: "sha256";
	permissions: "private";
}

export interface RetrievalConfig {
	lexical: boolean;
	fts: boolean;
	semantic: boolean;
	max_initial_results: number;
}

export interface PolicyConfig {
	version: 1;
	mode: PolicyMode;
	tool_output: ToolOutputConfig;
	artifacts: ArtifactConfig;
	retrieval: RetrievalConfig;
	ledger: { enabled: boolean; inject_every_turn: boolean };
	compaction: { use_harness_native: boolean; replace_native_compaction: boolean };
	reasoning: { auto_reduce: boolean };
	model_routing: { enabled: boolean };
	lossy_compression: { enabled: boolean };
	privacy: {
		local_only: boolean;
		redact_secrets: boolean;
		store_absolute_paths: boolean;
		store_full_commands: boolean;
		store_provider_session_ids: boolean;
		store_environment_values: boolean;
		allow_remote_telemetry: boolean;
	};
	telemetry: {
		enabled: boolean;
		store_prompt_content: boolean;
		store_model_response_content: boolean;
		store_tool_content: boolean;
	};
}

export const DEFAULT_CONFIG: PolicyConfig = {
	version: 1,
	mode: "observe",
	tool_output: {
		passthrough_bytes: 8192,
		structured_reduce_bytes: 32768,
		externalize_bytes: 32768,
		preserve_raw: true,
		fail_open: true,
		reducers: {
			tests: true,
			compiler: true,
			lint: true,
			git: true,
			package_manager: true,
			generic: false,
		},
	},
	artifacts: {
		enabled: true,
		root: ".token-policy",
		retention_days: 30,
		max_total_mb: 2048,
		hash: "sha256",
		permissions: "private",
	},
	retrieval: { lexical: true, fts: true, semantic: false, max_initial_results: 5 },
	ledger: { enabled: true, inject_every_turn: false },
	compaction: { use_harness_native: true, replace_native_compaction: false },
	reasoning: { auto_reduce: false },
	model_routing: { enabled: false },
	lossy_compression: { enabled: false },
	privacy: {
		local_only: true,
		redact_secrets: true,
		store_absolute_paths: false,
		store_full_commands: false,
		store_provider_session_ids: false,
		store_environment_values: false,
		allow_remote_telemetry: false,
	},
	telemetry: {
		enabled: true,
		store_prompt_content: false,
		store_model_response_content: false,
		store_tool_content: false,
	},
};

function cloneConfig(): PolicyConfig {
	return JSON.parse(JSON.stringify(DEFAULT_CONFIG)) as PolicyConfig;
}

function stripComment(value: string): string {
	let quote: '"' | "'" | undefined;
	for (let index = 0; index < value.length; index += 1) {
		const character = value[index];
		if ((character === '"' || character === "'") && value[index - 1] !== "\\") {
			quote = quote === character ? undefined : quote ?? character;
		}
		if (character === "#" && !quote && (index === 0 || /\s/.test(value[index - 1] ?? ""))) {
			return value.slice(0, index).trimEnd();
		}
	}
	return value.trim();
}

function parseScalar(value: string): unknown {
	const trimmed = stripComment(value).trim();
	if (trimmed === "") return {};
	if (trimmed === "true") return true;
	if (trimmed === "false") return false;
	if (trimmed === "null") return null;
	if (/^-?\d+(?:\.\d+)?$/.test(trimmed)) return Number(trimmed);
	if (trimmed.startsWith('"') && trimmed.endsWith('"')) {
		try {
			return JSON.parse(trimmed);
		} catch {
			return trimmed.slice(1, -1);
		}
	}
	if (trimmed.startsWith("'") && trimmed.endsWith("'")) return trimmed.slice(1, -1).replaceAll("''", "'");
	return trimmed;
}

/** Parses the small map-only YAML subset used by token-policy configuration. */
export function parseConfigText(text: string): Record<string, unknown> {
	const root: Record<string, unknown> = {};
	const stack: Array<{ indent: number; value: Record<string, unknown> }> = [{ indent: -1, value: root }];

	for (const rawLine of text.split(/\r?\n/)) {
		if (!rawLine.trim() || rawLine.trimStart().startsWith("#")) continue;
		const indent = rawLine.length - rawLine.trimStart().length;
		const match = rawLine.trim().match(/^([^:#]+):(?:\s*(.*))?$/);
		if (!match) continue;
		const key = match[1].trim();
		const valueText = match[2] ?? "";
		while (stack.length > 1 && stack[stack.length - 1].indent >= indent) stack.pop();
		const parent = stack[stack.length - 1].value;
		if (!valueText.trim()) {
			const child: Record<string, unknown> = {};
			parent[key] = child;
			stack.push({ indent, value: child });
		} else {
			parent[key] = parseScalar(valueText);
		}
	}
	return root;
}

function isRecord(value: unknown): value is Record<string, unknown> {
	return typeof value === "object" && value !== null && !Array.isArray(value);
}

function merge(base: Record<string, unknown>, override: Record<string, unknown>): Record<string, unknown> {
	const result: Record<string, unknown> = { ...base };
	for (const [key, value] of Object.entries(override)) {
		if (isRecord(value) && isRecord(result[key])) result[key] = merge(result[key] as Record<string, unknown>, value);
		else result[key] = value;
	}
	return result;
}

function positiveInteger(value: unknown, fallback: number): number {
	return typeof value === "number" && Number.isInteger(value) && value >= 0 ? value : fallback;
}

function booleanValue(value: unknown, fallback: boolean): boolean {
	return typeof value === "boolean" ? value : fallback;
}

function recordValue(value: unknown): Record<string, unknown> {
	return isRecord(value) ? value : {};
}

function normalize(raw: Record<string, unknown>, cwd = process.cwd()): PolicyConfig {
	const defaults = cloneConfig();
	const merged = merge(defaults as unknown as Record<string, unknown>, raw);
	const output = recordValue(merged.tool_output);
	const artifact = recordValue(merged.artifacts);
	const retrieval = recordValue(merged.retrieval);
	const ledger = recordValue(merged.ledger);
	const privacy = recordValue(merged.privacy);
	const telemetry = recordValue(merged.telemetry);
	const reducerValues = recordValue(output.reducers);
	const reducers: Record<string, boolean> = { ...defaults.tool_output.reducers };
	for (const [key, value] of Object.entries(reducerValues)) {
		if (typeof value === "boolean") reducers[key] = value;
	}
	const configuredRoot = typeof artifact.root === "string" ? artifact.root : defaults.artifacts.root;
	const rootIsSafe = !isAbsolute(configuredRoot) && !relative(cwd, resolve(cwd, configuredRoot)).startsWith("..");
	return {
		version: 1,
		mode: merged.mode === "safe" || merged.mode === "experimental" ? merged.mode : "observe",
		tool_output: {
			passthrough_bytes: positiveInteger(output.passthrough_bytes, defaults.tool_output.passthrough_bytes),
			structured_reduce_bytes: positiveInteger(output.structured_reduce_bytes, defaults.tool_output.structured_reduce_bytes),
			externalize_bytes: positiveInteger(output.externalize_bytes, defaults.tool_output.externalize_bytes),
			preserve_raw: booleanValue(output.preserve_raw, defaults.tool_output.preserve_raw),
			fail_open: booleanValue(output.fail_open, defaults.tool_output.fail_open),
			reducers,
		},
		artifacts: {
			enabled: booleanValue(artifact.enabled, defaults.artifacts.enabled),
			root: rootIsSafe ? configuredRoot : defaults.artifacts.root,
			retention_days: Math.max(1, positiveInteger(artifact.retention_days, defaults.artifacts.retention_days)),
			max_total_mb: Math.max(1, positiveInteger(artifact.max_total_mb, defaults.artifacts.max_total_mb)),
			hash: "sha256",
			permissions: "private",
		},
		retrieval: {
			lexical: booleanValue(retrieval.lexical, defaults.retrieval.lexical),
			fts: booleanValue(retrieval.fts, defaults.retrieval.fts),
			semantic: false,
			max_initial_results: Math.min(50, Math.max(1, positiveInteger(retrieval.max_initial_results, defaults.retrieval.max_initial_results))),
		},
		ledger: { enabled: booleanValue(ledger.enabled, true), inject_every_turn: false },
		compaction: { use_harness_native: true, replace_native_compaction: false },
		reasoning: { auto_reduce: false },
		model_routing: { enabled: false },
		lossy_compression: { enabled: false },
		privacy: {
			local_only: true,
			redact_secrets: true,
			store_absolute_paths: false,
			store_full_commands: false,
			store_provider_session_ids: false,
			store_environment_values: false,
			allow_remote_telemetry: false,
		},
		telemetry: {
			enabled: booleanValue(telemetry.enabled, defaults.telemetry.enabled),
			store_prompt_content: false,
			store_model_response_content: false,
			store_tool_content: false,
		},
	};
}

export function resolveRuntimeRoot(cwd: string, configuredRoot: string): string {
	if (isAbsolute(configuredRoot)) return resolve(cwd, ".token-policy");
	const candidate = resolve(cwd, configuredRoot);
	const rel = relative(cwd, candidate);
	return rel === "" || rel === ".." || rel.startsWith(`..${requireSeparator(cwd)}`) ? resolve(cwd, ".token-policy") : candidate;
}

function requireSeparator(_cwd: string): string {
	return process.platform === "win32" ? "\\" : "/";
}

export async function loadConfig(cwd: string): Promise<{ config: PolicyConfig; path?: string }> {
	const candidates = [
		resolve(cwd, ".pi", "token-policy.yaml"),
		resolve(cwd, ".pi", "token-policy.yml"),
		resolve(cwd, ".pi", "token-efficiency.yaml"),
	];
	for (const path of candidates) {
		try {
			const text = await readFile(path, "utf8");
			return { config: normalize(parseConfigText(text), cwd), path };
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") return { config: cloneConfig(), path };
		}
	}
	return { config: cloneConfig() };
}
