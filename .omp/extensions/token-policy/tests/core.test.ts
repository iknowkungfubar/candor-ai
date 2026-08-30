import { afterEach, describe, expect, it } from "bun:test";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, rm, stat, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { ArtifactStore } from "../artifacts.ts";
import { DEFAULT_CONFIG, parseConfigText } from "../config.ts";
import { createLedger, ledgerFromBranch, saveLedger, loadLedger, updateLedger } from "../ledger.ts";
import { classifyCommand } from "../reducers/classify.ts";
import { reduceOutput } from "../reducers/index.ts";
import { retrieveArtifact } from "../retrieval.ts";

const temporaryDirectories: string[] = [];

afterEach(async () => {
	await Promise.all(temporaryDirectories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

describe("config and classification", () => {
	it("parses the example map-only YAML without enabling experimental policies", () => {
		const config = parseConfigText("mode: safe\ntool_output:\n  reducers:\n    tests: false\n");
		expect(config.mode).toBe("safe");
		expect((config.tool_output as any).reducers.tests).toBe(false);
		expect(DEFAULT_CONFIG.reasoning.auto_reduce).toBe(false);
		expect(classifyCommand("pytest -q")).toBe("tests");
		expect(classifyCommand("git diff --stat")).toBe("git");
		expect(classifyCommand("cat build.log")).toBeUndefined();
	});
});

describe("reducers", () => {
	it("reduces recognized large output while retaining failure evidence", () => {
		const raw = `${"passing test\n".repeat(1000)}AssertionError: expected 1 to equal 2\n at test.ts:4\n`;
		const reduced = reduceOutput("tests", raw, { isError: true, artifactId: "token-artifact:test" });
		expect(reduced.text).toContain("AssertionError: expected 1 to equal 2");
		expect(Buffer.byteLength(reduced.text)).toBeLessThan(Buffer.byteLength(raw));
	});

	it("returns a thrown reducer failure to the caller", () => {
		expect(() => reduceOutput("tests", "output", { isError: false, artifactId: "token-artifact:test" }, { tests: () => { throw new Error("fixture failure"); } })).toThrow("fixture failure");
	});
});

describe("artifact storage and retrieval", () => {
	it("preserves raw UTF-8 output byte-for-byte and writes private metadata", async () => {
		const cwd = await mkdtemp(join(process.cwd(), ".token-policy-artifact-"));
		temporaryDirectories.push(cwd);
		const raw = "λ\r\nAssertionError: expected\n";
		const store = new ArtifactStore({ root: join(cwd, ".token-policy"), retentionDays: 30, maxTotalMb: 1 });
		const record = await store.save(raw, { runId: "run-test", tool: "bash", commandClass: "tests", modelVisibleBytes: 10 });
		expect(await store.readExact(record.metadata.id)).toBe(raw);
		expect(createHash("sha256").update(raw).digest("hex")).toBe(record.metadata.sha256);
		expect((await stat(join(cwd, ".token-policy"))).mode & 0o777).toBe(0o700);
		expect((await stat(join(cwd, ".token-policy", "artifacts", record.metadata.id.slice("token-artifact:".length), "stdout.txt"))).mode & 0o777).toBe(0o600);
		expect(await retrieveArtifact(store, { operation: "search", artifact: record.metadata.id, query: "Assertion" }, 5)).toContain("AssertionError");
	});

	it("rejects traversal-shaped artifact identifiers", async () => {
		const cwd = await mkdtemp(join(process.cwd(), ".token-policy-artifact-"));
		temporaryDirectories.push(cwd);
		const store = new ArtifactStore({ root: join(cwd, ".token-policy"), retentionDays: 30, maxTotalMb: 1 });
		await expect(store.readExact("token-artifact:../escape")).rejects.toThrow("invalid artifact id");
	});

	it("does not follow a symlinked runtime root", async () => {
		const cwd = await mkdtemp(join(process.cwd(), ".token-policy-artifact-"));
		temporaryDirectories.push(cwd);
		const outside = await mkdtemp(join(process.cwd(), ".token-policy-outside-"));
		temporaryDirectories.push(outside);
		await symlink(outside, join(cwd, ".token-policy"), "dir");
		const store = new ArtifactStore({ root: join(cwd, ".token-policy"), retentionDays: 30, maxTotalMb: 1 });
		await expect(store.save("raw", { runId: "run-test", tool: "bash", modelVisibleBytes: 3 })).rejects.toThrow("private directory");
	});
});

describe("ledger", () => {
	it("survives local persistence and compaction-entry reconstruction", async () => {
		const cwd = await mkdtemp(join(process.cwd(), ".token-policy-ledger-"));
		temporaryDirectories.push(cwd);
		const ledger = updateLedger(createLedger(), { current_phase: "before-compaction", next_action: "resume verification" });
		await saveLedger(join(cwd, ".token-policy"), ledger);
		expect((await loadLedger(join(cwd, ".token-policy")))?.next_action).toBe("resume verification");
		expect(ledgerFromBranch([{ type: "custom", customType: "token-policy.ledger.v1", data: ledger }])?.current_phase).toBe("before-compaction");
	});
});
