import { appendFile, chmod, mkdir, readFile } from "node:fs/promises";
import { join } from "node:path";

export type TelemetryEventType = "model_call" | "tool_result" | "artifact" | "compaction" | "session_summary" | "error";

export interface TelemetryEvent {
	schema_version: 1;
	event_type: TelemetryEventType;
	timestamp: string;
	provider?: string | null;
	model?: string | null;
	input_tokens?: number | null;
	cached_input_tokens?: number | null;
	cache_write_tokens?: number | null;
	reasoning_tokens?: number | null;
	output_tokens?: number | null;
	raw_tool_bytes?: number | null;
	model_visible_tool_bytes?: number | null;
	task_success?: boolean | null;
	run_id: string;
	[key: string]: unknown;
}

export class TelemetryStore {
	private readonly file: string;
	private initialized = false;

	constructor(private readonly root: string, private readonly enabled: boolean) {
		this.file = join(root, "telemetry.jsonl");
	}

	async initialize(): Promise<void> {
		if (!this.enabled || this.initialized) return;
		await mkdir(this.root, { recursive: true, mode: 0o700 });
		await chmod(this.root, 0o700);
		this.initialized = true;
	}

	async record(event: TelemetryEvent): Promise<void> {
		if (!this.enabled) return;
		await this.initialize();
		await appendFile(this.file, `${JSON.stringify(event)}\n`, { encoding: "utf8", mode: 0o600 });
		await chmod(this.file, 0o600);
	}

	async readEvents(): Promise<TelemetryEvent[]> {
		if (!this.enabled) return [];
		try {
			const content = await readFile(this.file, "utf8");
			return content
				.split(/\r?\n/)
				.filter(Boolean)
				.map((line) => JSON.parse(line) as TelemetryEvent);
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return [];
			throw error;
		}
	}
}

export function event(runId: string, eventType: TelemetryEventType, fields: Record<string, unknown> = {}): TelemetryEvent {
	return {
		schema_version: 1,
		event_type: eventType,
		timestamp: new Date().toISOString(),
		run_id: runId,
		...fields,
	};
}
