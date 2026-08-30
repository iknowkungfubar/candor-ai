import { afterEach, describe, expect, it } from "bun:test";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import tokenPolicy from "../index.ts";

const temporaryDirectories: string[] = [];

afterEach(async () => {
	await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

function fakePi() {
	const handlers = new Map<string, (event: any, ctx?: any) => unknown>();
	const tools = new Map<string, any>();
	let activeTools = ["bash", "read"];
	const appended: unknown[] = [];
	return {
		handlers,
		tools,
		appended,
		on(name: string, handler: (event: any, ctx?: any) => unknown) { handlers.set(name, handler); },
		registerCommand() {},
		registerTool(definition: any) { tools.set(definition.name, definition); },
		getActiveTools: () => activeTools,
		setActiveTools: (names: string[]) => { activeTools = names; },
		get activeTools() { return activeTools; },
		appendEntry(_type: string, data: unknown) { appended.push(data); },
	};
}

function context(cwd: string, signal?: AbortSignal) {
	return {
		cwd,
		mode: "print",
		model: undefined,
		signal,
		getContextUsage: () => undefined,
		sessionManager: { getBranch: () => [] },
	};
}

async function safeProject(config = "mode: safe\n") {
	const cwd = await mkdtemp(join(process.cwd(), ".token-policy-safe-"));
	temporaryDirectories.push(cwd);
	await mkdir(join(cwd, ".omp"), { recursive: true });
	await writeFile(join(cwd, ".omp", "token-policy.yaml"), config);
	return cwd;
}

function toolResult(command: string, text: string, isError = false, details: Record<string, unknown> = { native: "preserved" }) {
	return {
		type: "tool_result",
		toolName: "bash",
		toolCallId: "call-1",
		input: { command },
		content: [{ type: "text", text }],
		isError,
		details,
	};
}

describe("safe mode", () => {
	it("honors the passthrough threshold", async () => {
		const cwd = await safeProject("mode: safe\ntool_output:\n  passthrough_bytes: 100000\n");
		const pi = fakePi();
		tokenPolicy(pi as any);
		await pi.handlers.get("session_start")?.({}, context(cwd));
		expect(await pi.handlers.get("tool_result")?.(toolResult("pytest", "test\n".repeat(1000)), context(cwd))).toBeUndefined();
	});

	it("keeps small and unknown output unchanged", async () => {
		const cwd = await safeProject();
		const pi = fakePi();
		tokenPolicy(pi as any);
		await pi.handlers.get("session_start")?.({}, context(cwd));
		const small = toolResult("pytest", "ok\n");
		const unknown = toolResult("cat build.log", "unknown output\n".repeat(5000));
		expect(await pi.handlers.get("tool_result")?.(small, context(cwd))).toBeUndefined();
		expect(await pi.handlers.get("tool_result")?.(unknown, context(cwd))).toBeUndefined();
	});

	it("reduces allowlisted output, preserves error status, and retrieves exact raw output", async () => {
		const cwd = await safeProject();
		const pi = fakePi();
		tokenPolicy(pi as any);
		await pi.handlers.get("session_start")?.({}, context(cwd));
		const original = `${"passing test\n".repeat(5000)}AssertionError: expected 1 to equal 2\n`;
		const result = toolResult("pytest -q", original, true);
		const patch: any = await pi.handlers.get("tool_result")?.(result, context(cwd));

		expect(patch.content[0].text).toContain("AssertionError: expected 1 to equal 2");
		expect(Buffer.byteLength(patch.content[0].text)).toBeLessThan(Buffer.byteLength(original));
		expect(patch.isError).toBeUndefined();
		expect(result.isError).toBe(true);
		expect(patch.details.native).toBe("preserved");
		expect(patch.details.token_policy.artifact_id).toMatch(/^token-artifact:/);
		expect(patch.details.token_policy.model_path).not.toContain(cwd);
		expect(pi.activeTools).toContain("token_context");

		const exact = await readFile(join(cwd, patch.details.token_policy.model_path), "utf8");
		expect(exact).toBe(original);
		const retrieval = await pi.tools.get("token_context").execute("retrieve-1", {
			operation: "read",
			artifact: patch.details.token_policy.artifact_id,
			limit: 128,
		}, undefined, undefined, context(cwd));
		expect(retrieval.content[0].text).toBe(original.slice(0, 128));
		await expect(pi.tools.get("token_context").execute("retrieve-2", { operation: "metadata", artifact: "token-artifact:../escape" }, undefined, undefined, context(cwd))).rejects.toThrow();

		const telemetry = await readFile(join(cwd, ".token-policy", "telemetry.jsonl"), "utf8");
		expect(telemetry).not.toContain("AssertionError: expected 1 to equal 2");
	});

	it("copies Pi's native full output before reducing a truncated result", async () => {
		const cwd = await safeProject();
		const nativeDirectory = await mkdtemp(join(tmpdir(), "token-policy-native-"));
		temporaryDirectories.push(nativeDirectory);
		const nativePath = join(nativeDirectory, "native-full-output.txt");
		const original = `${"full test output\n".repeat(4000)}FAILED native failure\n`;
		await writeFile(nativePath, original);
		const pi = fakePi();
		tokenPolicy(pi as any);
		await pi.handlers.get("session_start")?.({}, context(cwd));
		const result = toolResult("pytest", "[native truncated output]", true, { fullOutputPath: nativePath });
		const patch: any = await pi.handlers.get("tool_result")?.(result, context(cwd));
		expect(patch.details.fullOutputPath).toBeUndefined();
		expect(await readFile(join(cwd, patch.details.token_policy.model_path), "utf8")).toBe(original);
	});

	it("does not transform an aborted result", async () => {
		const cwd = await safeProject();
		const pi = fakePi();
		tokenPolicy(pi as any);
		await pi.handlers.get("session_start")?.({}, context(cwd));
		const controller = new AbortController();
		controller.abort();
		const original = "error\n".repeat(10000);
		expect(await pi.handlers.get("tool_result")?.(toolResult("pytest", original, true), context(cwd, controller.signal))).toBeUndefined();
	});

	it("fails open when artifact storage is unavailable", async () => {
		const cwd = await safeProject();
		await writeFile(join(cwd, ".token-policy"), "not a directory\n");
		const pi = fakePi();
		tokenPolicy(pi as any);
		await pi.handlers.get("session_start")?.({}, context(cwd));
		const original = "error\n".repeat(10000);
		expect(await pi.handlers.get("tool_result")?.(toolResult("pytest", original, true), context(cwd))).toBeUndefined();
	});

	it("checkpoints concise ledger state around native compaction without injecting every turn", async () => {
		const cwd = await safeProject();
		const pi = fakePi();
		tokenPolicy(pi as any);
		await pi.handlers.get("session_start")?.({}, context(cwd));
		await pi.handlers.get("session_before_compact")?.({ reason: "threshold" }, context(cwd));
		const ledger = JSON.parse(await readFile(join(cwd, ".token-policy", "ledger.json"), "utf8"));
		expect(ledger.current_phase).toBe("before-compaction");
		expect(pi.appended).toHaveLength(1);
		expect(pi.appended[0]).toMatchObject({ current_phase: "before-compaction" });
		expect(pi.handlers.has("context")).toBe(false);
		await pi.handlers.get("session_compact")?.({ reason: "threshold", fromExtension: false }, context(cwd));
		const after = JSON.parse(await readFile(join(cwd, ".token-policy", "ledger.json"), "utf8"));
		expect(after.current_phase).toBe("resumed-after-compaction");
	});
});
