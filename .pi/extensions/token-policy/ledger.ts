import { randomUUID } from "node:crypto";
import { chmod, mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { join } from "node:path";

export const LEDGER_ENTRY_TYPE = "token-policy.ledger.v1";

export interface ModifiedFile {
	path: string;
	state: string;
	hash?: string | null;
}

export interface ExecutionLedger {
	schema_version: 1;
	objective: string;
	current_phase: string;
	constraints: string[];
	decisions: string[];
	modified_files: ModifiedFile[];
	tests: { last_command: string | null; status: string | null; artifact: string | null };
	known_failures: string[];
	rejected_approaches: string[];
	open_questions: string[];
	next_action: string;
	updated_at: string;
}

export function createLedger(now = new Date().toISOString()): ExecutionLedger {
	return {
		schema_version: 1,
		objective: "active agent task",
		current_phase: "starting",
		constraints: ["preserve correctness", "fail open", "keep native compaction"],
		decisions: [],
		modified_files: [],
		tests: { last_command: null, status: null, artifact: null },
		known_failures: [],
		rejected_approaches: ["global reasoning reduction", "lossy prompt compression", "automatic model routing"],
		open_questions: [],
		next_action: "continue current task",
		updated_at: now,
	};
}

export function isExecutionLedger(value: unknown): value is ExecutionLedger {
	if (!value || typeof value !== "object") return false;
	const ledger = value as Partial<ExecutionLedger>;
	return ledger.schema_version === 1 && typeof ledger.objective === "string" && typeof ledger.current_phase === "string" &&
		Array.isArray(ledger.constraints) && Array.isArray(ledger.decisions) && Array.isArray(ledger.modified_files) &&
		ledger.tests !== undefined && Array.isArray(ledger.known_failures) && Array.isArray(ledger.rejected_approaches) &&
		Array.isArray(ledger.open_questions) && typeof ledger.next_action === "string" && typeof ledger.updated_at === "string";
}

export function updateLedger(ledger: ExecutionLedger, patch: Partial<ExecutionLedger>): ExecutionLedger {
	return { ...ledger, ...patch, updated_at: new Date().toISOString() };
}

export function addModifiedFile(ledger: ExecutionLedger, path: string, state: string): ExecutionLedger {
	if (ledger.modified_files.some((file) => file.path === path)) return updateLedger(ledger, { current_phase: "working" });
	return updateLedger(ledger, {
		current_phase: "working",
		modified_files: [...ledger.modified_files, { path, state }],
		next_action: "verify changes",
	});
}

export async function saveLedger(root: string, ledger: ExecutionLedger): Promise<void> {
	await mkdir(root, { recursive: true, mode: 0o700 });
	await chmod(root, 0o700);
	const path = join(root, "ledger.json");
	const temporaryPath = join(root, `.ledger-${randomUUID()}.tmp`);
	await writeFile(temporaryPath, `${JSON.stringify(ledger, null, 2)}\n`, { encoding: "utf8", mode: 0o600 });
	await chmod(temporaryPath, 0o600);
	await rename(temporaryPath, path);
	await chmod(path, 0o600);
}

export async function loadLedger(root: string): Promise<ExecutionLedger | undefined> {
	try {
		const value: unknown = JSON.parse(await readFile(join(root, "ledger.json"), "utf8"));
		return isExecutionLedger(value) ? value : undefined;
	} catch (error) {
		if ((error as NodeJS.ErrnoException).code === "ENOENT") return undefined;
		return undefined;
	}
}

export function ledgerFromBranch(entries: readonly unknown[]): ExecutionLedger | undefined {
	let latest: ExecutionLedger | undefined;
	for (const entry of entries) {
		if (!entry || typeof entry !== "object") continue;
		const candidate = entry as { type?: string; customType?: string; data?: unknown };
		if (candidate.type !== "custom" || candidate.customType !== LEDGER_ENTRY_TYPE || !isExecutionLedger(candidate.data)) continue;
		if (!latest || candidate.data.updated_at >= latest.updated_at) latest = candidate.data;
	}
	return latest;
}
