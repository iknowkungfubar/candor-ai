export type OutputClass = "tests" | "compiler" | "lint" | "git" | "package_manager";

function commandName(command: string): string | undefined {
	const match = command.trim().match(/^(?:(?:[A-Za-z_][A-Za-z0-9_]*=[^\s]+)\s+)*(?:command\s+)?(?:[^\s/]+\/)*([^\s]+)/);
	return match?.[1]?.toLowerCase();
}

export function classifyCommand(command: string): OutputClass | undefined {
	const trimmed = command.trim();
	const name = commandName(trimmed);
	if (!name) return undefined;

	if (/^(?:npm|pnpm|yarn|bun)\s+(?:(?:run|exec)\s+)?(?:test|tests)(?:\s|$)/i.test(trimmed)) return "tests";
	if (/^(?:pytest|vitest|jest|cargo\s+test|go\s+test|python\s+-m\s+pytest)(?:\s|$)/i.test(trimmed)) return "tests";
	if (/^(?:npm|pnpm|yarn|bun)\s+(?:(?:run|exec)\s+)?(?:build|compile)(?:\s|$)/i.test(trimmed)) return "compiler";
	if (/^(?:tsc|npx\s+tsc|cargo\s+(?:check|build)|go\s+build|mypy|pyright)(?:\s|$)/i.test(trimmed)) return "compiler";
	if (/^(?:npm|pnpm|yarn|bun)\s+(?:(?:run|exec)\s+)?lint(?:\s|$)/i.test(trimmed)) return "lint";
	if (/^(?:eslint|biome|prettier\s+--check|ruff|clippy)(?:\s|$)/i.test(trimmed)) return "lint";
	if (/^git\s+(?:status|diff\s+--(?:stat|shortstat|name-only|name-status)|log\s+--oneline)(?:\s|$)/i.test(trimmed)) return "git";
	if (/^(?:npm|pnpm|yarn|bun)\s+(?:install|ci|audit|outdated|list)(?:\s|$)/i.test(trimmed)) return "package_manager";
	return undefined;
}
