import { createHash, randomUUID } from "node:crypto";
import { chmod, lstat, mkdir, readFile, readdir, rm, stat, writeFile } from "node:fs/promises";
import { dirname, join, posix, relative, resolve } from "node:path";

export interface ArtifactMetadata {
	id: string;
	timestamp: string;
	run_id: string;
	turn_id?: string | null;
	tool: string;
	command_class?: string | null;
	repo_relative_cwd?: string | null;
	exit_code?: number | null;
	stdout_path?: string | null;
	stderr_path?: string | null;
	stdout_bytes: number;
	stderr_bytes: number;
	sha256: string;
	reducer?: string | null;
	model_visible_bytes: number;
}

export interface ArtifactRecord {
	metadata: ArtifactMetadata;
	modelPath: string;
}

export interface ArtifactStoreOptions {
	root: string;
	/** Repository-relative label used in model-visible recovery instructions. */
	modelRoot?: string;
	retentionDays: number;
	maxTotalMb: number;
}

export interface StoreArtifactInput {
	runId: string;
	turnId?: string;
	tool: string;
	commandClass?: string;
	repoRelativeCwd?: string;
	modelVisibleBytes: number;
	exitCode?: number | null;
}

const ARTIFACT_PREFIX = "token-artifact:";
const SAFE_ID = /^[A-Za-z0-9_-]{8,128}$/;

function throwIfAborted(signal?: AbortSignal): void {
	if (signal?.aborted) throw new Error("operation cancelled");
}

async function ensureDirectory(path: string, signal?: AbortSignal): Promise<void> {
	throwIfAborted(signal);
	const missing: string[] = [];
	let current = resolve(path);
	while (true) {
		try {
			const info = await lstat(current);
			if (info.isSymbolicLink() || !info.isDirectory()) throw new Error("artifact path is not a private directory");
			break;
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== "ENOENT") throw error;
			missing.push(current);
			const parent = dirname(current);
			if (parent === current) throw new Error("artifact path has no directory parent");
			current = parent;
		}
	}
	for (const directory of missing.reverse()) {
		throwIfAborted(signal);
		await mkdir(directory, { mode: 0o700 }).catch((mkdirError: NodeJS.ErrnoException) => {
			if (mkdirError.code !== "EEXIST") throw mkdirError;
		});
		const info = await lstat(directory);
		if (info.isSymbolicLink() || !info.isDirectory()) throw new Error("artifact path is not a private directory");
	}
	await chmod(path, 0o700);
}

async function ensureRegularFile(path: string): Promise<void> {
	const info = await lstat(path);
	if (info.isSymbolicLink() || !info.isFile()) throw new Error("artifact file is not regular");
}

export class ArtifactStore {
	private readonly artifactsRoot: string;
	private readonly inFlight = new Set<string>();
	private maintenance: Promise<void> = Promise.resolve();

	constructor(private readonly options: ArtifactStoreOptions) {
		this.artifactsRoot = join(options.root, "artifacts");
	}

	async initialize(signal?: AbortSignal): Promise<void> {
		await ensureDirectory(this.options.root, signal);
		await ensureDirectory(this.artifactsRoot, signal);
	}

	async cleanup(): Promise<void> {
		const protectedSlugs = new Set(this.inFlight);
		this.maintenance = this.maintenance.then(() => this.prune(protectedSlugs)).catch(() => undefined);
		await this.maintenance;
	}

	async save(rawText: string, input: StoreArtifactInput, signal?: AbortSignal): Promise<ArtifactRecord> {
		const id = `${ARTIFACT_PREFIX}${randomUUID()}`;
		const slug = this.slug(id);
		this.inFlight.add(slug);
		const directory = join(this.artifactsRoot, slug);
		const rawPath = join(directory, "stdout.txt");
		const metadataPath = join(directory, "metadata.json");
		const modelPath = posix.join(this.relativeRoot(), "artifacts", slug, "stdout.txt");
		const metadata: ArtifactMetadata = {
			id,
			timestamp: new Date().toISOString(),
			run_id: input.runId,
			turn_id: input.turnId ?? null,
			tool: input.tool,
			command_class: input.commandClass ?? null,
			repo_relative_cwd: input.repoRelativeCwd ?? ".",
			exit_code: input.exitCode ?? null,
			stdout_path: posix.join("artifacts", slug, "stdout.txt"),
			stderr_path: null,
			stdout_bytes: Buffer.byteLength(rawText, "utf8"),
			stderr_bytes: 0,
			sha256: createHash("sha256").update(rawText, "utf8").digest("hex"),
			reducer: null,
			model_visible_bytes: input.modelVisibleBytes,
		};

		try {
			throwIfAborted(signal);
			await this.initialize(signal);
			await mkdir(directory, { mode: 0o700 }).catch((error: NodeJS.ErrnoException) => {
				if (error.code !== "EEXIST") throw error;
			});
			await ensureDirectory(directory, signal);
			await writeFile(rawPath, rawText, { encoding: "utf8", mode: 0o600, signal });
			await ensureRegularFile(rawPath);
			await chmod(rawPath, 0o600);
			await writeFile(metadataPath, `${JSON.stringify(metadata, null, 2)}\n`, { encoding: "utf8", mode: 0o600, signal });
			await ensureRegularFile(metadataPath);
			await chmod(metadataPath, 0o600);
			await this.cleanup();
			return { metadata, modelPath };
		} catch (error) {
			await rm(directory, { recursive: true, force: true }).catch(() => undefined);
			throw error;
		} finally {
			this.inFlight.delete(slug);
		}
	}

	async updateMetadata(id: string, patch: Pick<ArtifactMetadata, "reducer" | "model_visible_bytes">, signal?: AbortSignal): Promise<ArtifactMetadata> {
		throwIfAborted(signal);
		const metadataPath = join(await this.safeDirectory(id), "metadata.json");
		await ensureRegularFile(metadataPath);
		const metadata = JSON.parse(await readFile(metadataPath, { encoding: "utf8", signal })) as ArtifactMetadata;
		const updated = { ...metadata, ...patch };
		throwIfAborted(signal);
		await writeFile(metadataPath, `${JSON.stringify(updated, null, 2)}\n`, { encoding: "utf8", mode: 0o600, signal });
		await chmod(metadataPath, 0o600);
		return updated;
	}

	async readExact(id: string, offset = 0, limit?: number, signal?: AbortSignal): Promise<string> {
		if (!Number.isInteger(offset) || offset < 0 || (limit !== undefined && (!Number.isInteger(limit) || limit < 0))) {
			throw new Error("offset and limit must be non-negative integers");
		}
		const rawPath = join(await this.safeDirectory(id), "stdout.txt");
		await ensureRegularFile(rawPath);
		const bytes = await readFile(rawPath, { signal });
		throwIfAborted(signal);
		const end = limit === undefined ? bytes.length : Math.min(bytes.length, offset + limit);
		return bytes.subarray(Math.min(offset, bytes.length), end).toString("utf8");
	}

	async readMetadata(id: string, signal?: AbortSignal): Promise<ArtifactMetadata> {
		const metadataPath = join(await this.safeDirectory(id), "metadata.json");
		await ensureRegularFile(metadataPath);
		return JSON.parse(await readFile(metadataPath, { encoding: "utf8", signal })) as ArtifactMetadata;
	}

	async search(id: string, query: string, maxResults: number, signal?: AbortSignal): Promise<Array<{ line: number; text: string }>> {
		if (!query) return [];
		const text = await this.readExact(id, 0, undefined, signal);
		const needle = query.toLocaleLowerCase();
		const results: Array<{ line: number; text: string }> = [];
		for (const [index, line] of text.split(/\r?\n/).entries()) {
			throwIfAborted(signal);
			if (!line.toLocaleLowerCase().includes(needle)) continue;
			results.push({ line: index + 1, text: line });
			if (results.length >= Math.max(1, Math.min(maxResults, 50))) break;
		}
		return results;
	}

	private async safeDirectory(id: string): Promise<string> {
		const directory = this.directory(id);
		await ensureDirectory(directory);
		return directory;
	}

	private directory(id: string): string {
		const slug = this.slug(id);
		const directory = resolve(this.artifactsRoot, slug);
		const rel = relative(this.artifactsRoot, directory);
		if (rel !== slug || rel.startsWith("..")) throw new Error("invalid artifact id");
		return directory;
	}

	private slug(id: string): string {
		if (!id.startsWith(ARTIFACT_PREFIX)) throw new Error("invalid artifact id");
		const slug = id.slice(ARTIFACT_PREFIX.length);
		if (!SAFE_ID.test(slug)) throw new Error("invalid artifact id");
		return slug;
	}

	private relativeRoot(): string {
		const configured = this.options.modelRoot ?? ".token-policy";
		const root = configured.replaceAll("\\", "/");
		if (!root || root.startsWith("/") || root.split("/").includes("..")) return ".token-policy";
		const normalized = posix.normalize(root);
		return normalized === "." ? ".token-policy" : normalized.replace(/^\.\//, "");
	}

	private async prune(protectedSlugs: ReadonlySet<string> = new Set()): Promise<void> {
		const cutoff = Date.now() - this.options.retentionDays * 24 * 60 * 60 * 1000;
		const entries = await readdir(this.artifactsRoot, { withFileTypes: true });
		const candidates: Array<{ slug: string; path: string; modified: number; bytes: number }> = [];
		for (const entry of entries) {
			if (!entry.isDirectory() || !SAFE_ID.test(entry.name)) continue;
			const path = join(this.artifactsRoot, entry.name);
			const info = await stat(path).catch(() => undefined);
			if (!info) continue;
			const bytes = await this.directorySize(path);
			candidates.push({ slug: entry.name, path, modified: info.mtimeMs, bytes });
			if (info.mtimeMs < cutoff && !protectedSlugs.has(entry.name)) await rm(path, { recursive: true, force: true });
		}
		const remaining = candidates.filter((item) => item.modified >= cutoff).sort((a, b) => a.modified - b.modified);
		let total = remaining.reduce((sum, item) => sum + item.bytes, 0);
		const maximum = this.options.maxTotalMb * 1024 * 1024;
		for (const item of remaining) {
			if (total <= maximum) break;
			if (protectedSlugs.has(item.slug)) continue;
			await rm(item.path, { recursive: true, force: true });
			total -= item.bytes;
		}
	}

	private async directorySize(path: string): Promise<number> {
		const entries = await readdir(path, { withFileTypes: true });
		let total = 0;
		for (const entry of entries) {
			const child = join(path, entry.name);
			const info = await lstat(child);
			if (info.isSymbolicLink()) continue;
			if (info.isDirectory()) total += await this.directorySize(child);
			else total += info.size;
		}
		return total;
	}
}

export function artifactReference(record: ArtifactRecord): string {
	return record.metadata.id;
}
