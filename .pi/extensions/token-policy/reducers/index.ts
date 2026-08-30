import type { OutputClass } from "./classify.ts";

export interface ReductionContext {
	isError: boolean;
	artifactId: string;
}

export type Reducer = (text: string, context: ReductionContext) => string;
export type ReducerMap = Partial<Record<OutputClass, Reducer>>;

function selectedLines(text: string, pattern: RegExp, head = 4, tail = 8, max = 120): string {
	const lines = text.split(/\r?\n/);
	const selected = new Set<number>();
	for (let index = 0; index < Math.min(head, lines.length); index += 1) selected.add(index);
	for (let index = Math.max(0, lines.length - tail); index < lines.length; index += 1) selected.add(index);
	for (let index = 0; index < lines.length; index += 1) {
		if (!pattern.test(lines[index])) continue;
		for (let nearby = Math.max(0, index - 1); nearby <= Math.min(lines.length - 1, index + 1); nearby += 1) selected.add(nearby);
	}
	const indexes = [...selected].sort((a, b) => a - b);
	const limited = indexes.length <= max ? indexes : [...indexes.slice(0, Math.floor(max / 2)), ...indexes.slice(-Math.ceil(max / 2))];
	const output = limited.map((index) => lines[index]);
	if (indexes.length > limited.length) output.splice(Math.floor(max / 2), 0, `[${indexes.length - limited.length} lines omitted]`);
	return output.join("\n").trim();
}

function tests(text: string, context: ReductionContext): string {
	return selectedLines(text, /(?:FAIL|FAILED|ERROR|AssertionError|Expected|Received|Test Suites?|Tests?|Snapshots?|Time|Ran|coverage|pass|fail)/i);
}

function compiler(text: string, _context: ReductionContext): string {
	return selectedLines(text, /(?:error|warning|TS\d+|typeerror|syntaxerror|failed|failure|cannot find|undefined)/i);
}

function lint(text: string, _context: ReductionContext): string {
	return selectedLines(text, /(?:error|warning|problem|✖|eslint|biome|ruff|prettier|failed)/i);
}

function git(text: string, _context: ReductionContext): string {
	return selectedLines(text, /(?:^\s*[MADRCU?!#]|changed|insertions?|deletions?|ahead|behind|nothing to commit|branch)/i, 12, 20, 100);
}

function packageManager(text: string, _context: ReductionContext): string {
	return selectedLines(text, /(?:ERR!|error|warning|added|removed|changed|audited|vulnerabilit|packages?|deprecated|failed)/i);
}

export const DEFAULT_REDUCERS: Record<OutputClass, Reducer> = {
	tests,
	compiler,
	lint,
	git,
	package_manager: packageManager,
};

export function reduceOutput(
	outputClass: OutputClass,
	text: string,
	context: ReductionContext,
	reducers: ReducerMap = DEFAULT_REDUCERS,
): { name: string; text: string } {
	const reducer = reducers[outputClass];
	if (!reducer) throw new Error(`no reducer for ${outputClass}`);
	const reduced = reducer(text, context);
	if (!reduced.trim()) throw new Error(`reducer ${outputClass} returned empty output`);
	return { name: `builtin:${outputClass}`, text: reduced };
}
