import type { ArtifactMetadata, ArtifactStore } from "./artifacts.ts";

export type RetrievalOperation = "read" | "search" | "metadata";

export interface RetrievalRequest {
	operation: RetrievalOperation;
	artifact: string;
	query?: string;
	offset?: number;
	limit?: number;
}

export async function retrieveArtifact(
	store: ArtifactStore,
	request: RetrievalRequest,
	maxResults: number,
	signal?: AbortSignal,
): Promise<string> {
	if (signal?.aborted) throw new Error("operation cancelled");
	switch (request.operation) {
		case "read":
			return store.readExact(request.artifact, request.offset ?? 0, request.limit ?? 8192, signal);
		case "search":
			return JSON.stringify({ artifact: request.artifact, matches: await store.search(request.artifact, request.query ?? "", maxResults, signal) }, null, 2);
		case "metadata":
			return JSON.stringify(await store.readMetadata(request.artifact, signal) as ArtifactMetadata, null, 2);
		default:
			throw new Error("unsupported retrieval operation");
	}
}
