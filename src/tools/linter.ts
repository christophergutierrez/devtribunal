import { exec } from "node:child_process";
import type { RecommendedTool, LinterFinding } from "../types.js";

const DEFAULT_TIMEOUT_MS = 30_000;

function checkInstalled(tool: RecommendedTool, timeoutMs = 5000): Promise<boolean> {
  if (!tool.check) return Promise.resolve(false);
  return new Promise((resolve) => {
    exec(tool.check, { timeout: timeoutMs }, (err) => {
      resolve(!err);
    });
  });
}

function execLinter(
  command: string,
  timeoutMs: number
): Promise<{ stdout: string; stderr: string; exitCode: number }> {
  return new Promise((resolve) => {
    exec(command, { timeout: timeoutMs, maxBuffer: 10 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err && err.killed) {
        // Process was killed (timeout)
        resolve({ stdout: "", stderr: "Linter timed out", exitCode: -1 });
        return;
      }
      if (err && !("code" in err)) {
        // Exec error (command not found, etc.)
        resolve({ stdout: "", stderr: String(err), exitCode: -1 });
        return;
      }
      // Linters often exit non-zero when findings exist — that's normal
      const exitCode = err?.code ?? 0;
      resolve({ stdout: stdout ?? "", stderr: stderr ?? "", exitCode });
    });
  });
}

function tryParseJsonFindings(
  tool: RecommendedTool,
  stdout: string
): LinterFinding[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(stdout);
  } catch {
    // JSON parse failed — return raw output as single finding
    if (stdout.trim()) {
      return [{
        tool: tool.name,
        file: "",
        line: null,
        column: null,
        severity: "info",
        message: stdout.trim(),
        rule: null,
      }];
    }
    return [];
  }

  const findings: LinterFinding[] = [];

  // Try common JSON linter output patterns
  const items = extractItems(parsed);
  for (const item of items) {
    if (typeof item !== "object" || item === null) continue;
    const obj = item as Record<string, unknown>;

    findings.push({
      tool: tool.name,
      file: String(obj.file ?? obj.path ?? obj.filename ?? obj.filePath ?? ""),
      line: toNumber(obj.line ?? obj.row ?? obj.startLine ?? obj.begin_line),
      column: toNumber(obj.column ?? obj.col ?? obj.startColumn ?? obj.begin_column),
      severity: String(obj.severity ?? obj.type ?? obj.level ?? "warning").toLowerCase(),
      message: String(obj.message ?? obj.msg ?? obj.description ?? ""),
      rule: obj.rule ? String(typeof obj.rule === "object" ? (obj.rule as Record<string, unknown>).id ?? obj.rule : obj.rule) : (obj.ruleId ? String(obj.ruleId) : (obj.code ? String(obj.code) : null)),
    });
  }

  return findings;
}

/**
 * Extract an array of finding items from various JSON structures.
 * Linters output in many shapes — try common patterns.
 */
function extractItems(parsed: unknown): unknown[] {
  // Direct array: [{...}, {...}]
  if (Array.isArray(parsed)) return parsed;

  if (typeof parsed === "object" && parsed !== null) {
    const obj = parsed as Record<string, unknown>;

    // { results: [{messages: [...]}] } (eslint format)
    if (Array.isArray(obj.results)) {
      const nested: unknown[] = [];
      for (const result of obj.results) {
        if (typeof result === "object" && result !== null) {
          const r = result as Record<string, unknown>;
          const filePath = r.filePath ?? r.file ?? "";
          const messages = Array.isArray(r.messages) ? r.messages : [];
          for (const msg of messages) {
            if (typeof msg === "object" && msg !== null) {
              nested.push({ ...msg as Record<string, unknown>, file: filePath });
            }
          }
        }
      }
      return nested;
    }

    // { diagnostics: [...] } (biome, some tools)
    if (Array.isArray(obj.diagnostics)) return obj.diagnostics;

    // { errors: [...] } (phpstan, etc.)
    if (Array.isArray(obj.errors)) return obj.errors;

    // { issues: [...] } (golangci-lint)
    if (Array.isArray(obj.issues)) return obj.issues;
  }

  return [];
}

function toNumber(val: unknown): number | null {
  if (typeof val === "number" && !isNaN(val)) return val;
  if (typeof val === "string") {
    const n = parseInt(val, 10);
    if (!isNaN(n)) return n;
  }
  return null;
}

function parseTextOutput(tool: RecommendedTool, stdout: string, stderr: string): LinterFinding[] {
  const output = (stdout + "\n" + stderr).trim();
  if (!output) return [];

  return [{
    tool: tool.name,
    file: "",
    line: null,
    column: null,
    severity: "info",
    message: output,
    rule: null,
  }];
}

function parseLinterOutput(
  tool: RecommendedTool,
  stdout: string,
  stderr: string
): LinterFinding[] {
  if (!tool.output_format) return [];

  if (tool.output_format === "json") {
    return tryParseJsonFindings(tool, stdout);
  }

  if (tool.output_format === "text") {
    return parseTextOutput(tool, stdout, stderr);
  }

  // Unknown format — treat as text
  return parseTextOutput(tool, stdout, stderr);
}

export interface LinterRunResult {
  findings: LinterFinding[];
  skipped: string[];
  errors: string[];
}

export async function runLinters(
  filePath: string,
  tools: RecommendedTool[],
  timeoutMs = DEFAULT_TIMEOUT_MS
): Promise<LinterRunResult> {
  const findings: LinterFinding[] = [];
  const skipped: string[] = [];
  const errors: string[] = [];

  // Filter to tools with run commands
  const runnableTools = tools.filter((t) => t.run);

  for (const tool of runnableTools) {
    // Check if installed
    const installed = await checkInstalled(tool);
    if (!installed) {
      skipped.push(tool.name);
      continue;
    }

    // Substitute {file} placeholder — quote the path for shell safety
    const command = tool.run.replace(/\{file\}/g, `"${filePath}"`);

    // Execute
    const result = await execLinter(command, timeoutMs);

    if (result.exitCode === -1) {
      errors.push(`${tool.name}: ${result.stderr}`);
      continue;
    }

    // Parse output
    const toolFindings = parseLinterOutput(tool, result.stdout, result.stderr);
    findings.push(...toolFindings);
  }

  return { findings, skipped, errors };
}
