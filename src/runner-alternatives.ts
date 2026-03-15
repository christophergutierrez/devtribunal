/**
 * Package runner alternatives for cross-ecosystem tool detection.
 * When a command uses a known runner prefix (npx, pip, etc.),
 * alternatives are tried in order until one succeeds.
 */

// Tried in order — put the most common alternative first
const RUNNER_ALTERNATIVES: Record<string, string[]> = {
  npx: ["bunx", "pnpx", "npx"],
  pip: ["uv pip", "pip", "conda run pip"],
};

/**
 * Expand a command into alternatives for different package runners.
 * e.g. "npx eslint --version" → ["bunx eslint --version", "pnpx eslint --version", "npx eslint --version"]
 * Commands without a known runner prefix are returned as-is.
 */
export function expandRunnerAlternatives(
  cmd: string
): Array<{ cmd: string; runner: string }> {
  for (const [prefix, alternatives] of Object.entries(RUNNER_ALTERNATIVES)) {
    if (cmd.startsWith(prefix + " ")) {
      const rest = cmd.slice(prefix.length + 1);
      return alternatives.map((alt) => ({ cmd: `${alt} ${rest}`, runner: alt }));
    }
  }
  return [{ cmd, runner: "system" }];
}
