import { execFile as nodeExecFile } from "node:child_process";

/**
 * Validate a file path for shell safety.
 * Rejects paths containing characters that could be used for command injection.
 */
export function validateFilePath(filePath: string): void {
  if (/[`$"';&|!\n\r\0]/.test(filePath)) {
    throw new Error(
      `Unsafe file path rejected: path contains shell metacharacters. ` +
      `Path: ${filePath.slice(0, 200)}`
    );
  }
}

/**
 * Split a command string into binary and arguments.
 * Only safe for known command templates from agent definitions
 * (e.g. "npx eslint --format json {file}"), NOT for arbitrary user input.
 */
export function splitCommand(cmd: string): { bin: string; args: string[] } {
  const parts = cmd.trim().split(/\s+/);
  return { bin: parts[0], args: parts.slice(1) };
}

/**
 * Safe wrapper around execFile that returns a promise.
 * Does NOT invoke a shell — arguments are passed directly to the process.
 */
export function safeExecFile(
  bin: string,
  args: string[],
  options: { timeout?: number; maxBuffer?: number } = {}
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  return new Promise((resolve) => {
    nodeExecFile(bin, args, {
      timeout: options.timeout ?? 30_000,
      maxBuffer: options.maxBuffer ?? 10 * 1024 * 1024,
    }, (err, stdout, stderr) => {
      if (err && "killed" in err && err.killed) {
        resolve({ stdout: "", stderr: "Process timed out", exitCode: -1 });
        return;
      }
      if (err && !("code" in err)) {
        resolve({ stdout: "", stderr: String(err), exitCode: -1 });
        return;
      }
      // execFile error has numeric exit code when process exits non-zero
      const exitCode = err && typeof (err as { code?: unknown }).code === "number"
        ? (err as { code: number }).code
        : err ? 1 : 0;
      resolve({ stdout: stdout ?? "", stderr: stderr ?? "", exitCode });
    });
  });
}
