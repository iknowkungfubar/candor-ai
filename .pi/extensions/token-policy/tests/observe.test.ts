import { afterEach, describe, expect, it } from "bun:test";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import tokenPolicy from "../index.ts";

const temporaryDirectories: string[] = [];

afterEach(async () => {
	await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

function fakePi() {
	const handlers = new Map<string, (event: any, ctx: any) => unknown>();
	let activeTools = ["bash", "read"];
	return {
		handlers,
		on(name: string, handler: (event: any, ctx: any) => unknown) {
			handlers.set(name, handler);
		},
		registerCommand() {},
		registerTool() {},
		getActiveTools: () => activeTools,
		setActiveTools: (names: string[]) => { activeTools = names; },
		appendEntry() {},
	};
}

function context(cwd: string) {
	return {
		cwd,
		mode: "print",
		model: undefined,
		getContextUsage: () => undefined,
	};
}

describe("observe mode", () => {
	it("leaves large tool output unchanged and records content-free baseline metrics", async () => {
		const cwd = await mkdtemp(join(process.cwd(), ".token-policy-observe-"));
		temporaryDirectories.push(cwd);
		const configDirectory = join(cwd, ".pi");
		await mkdir(configDirectory, { recursive: true });
		await writeFile(join(configDirectory, "token-policy.yaml"), "mode: observe\n");

		const pi = fakePi();
		tokenPolicy(pi as any);
		await pi.handlers.get("session_start")?.({}, context(cwd));
		const original = `${"test line\n".repeat(5000)}FAILED exact failure\n`;
		const result = await pi.handlers.get("tool_result")?.({
			type: "tool_result",
			toolName: "bash",
			toolCallId: "call-1",
			input: { command: "pytest" },
			content: [{ type: "text", text: original }],
			isError: true,
		}, context(cwd));
		expect(result).toBeUndefined();
		await pi.handlers.get("message_end")?.({
			message: {
				role: "assistant",
				provider: "synthetic",
				model: "synthetic-model",
				usage: { input: 10, output: 4, cacheRead: 2, cacheWrite: 0, reasoning: 0 },
			},
		}, context(cwd));
		const telemetry = await readFile(join(cwd, ".token-policy", "telemetry.jsonl"), "utf8");
		const records = telemetry.trim().split(/\r?\n/).map((line) => JSON.parse(line));
		const record = records.find((entry) => entry.event_type === "tool_result");
		expect(record.raw_tool_bytes).toBe(Buffer.byteLength(original));
		expect(record.model_visible_tool_bytes).toBe(record.raw_tool_bytes);
		expect(record).not.toHaveProperty("content");
		expect(record).not.toHaveProperty("command");
		expect(records.find((entry) => entry.event_type === "model_call")).toMatchObject({ input_tokens: 10, output_tokens: 4, cached_input_tokens: 2 });
	});
});
