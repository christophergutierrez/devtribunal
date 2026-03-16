import type { RecommendedTool, LinterFinding } from "../types.js";
import { expandRunnerAlternatives } from "../runner-alternatives.js";
import { validateFilePath, splitCommand, safeExecFile } from "../utils/shell.js";

const DEFAULT_TIMEOUT_MS = 30_000;

/**
 * Check if a tool is installed, trying package-runner alternatives.
 * Returns the runner prefix that works (e.g. "bunx"), or null if not found.
 */
async function checkInstalled(
  tool: RecommendedTool,
  timeoutMs = 5000
): Promise<string | null> {
  if (!tool.check) return null;

  const alternatives = expandRunnerAlternatives(tool.check);
  for (const alt of alternatives) {
    const { bin, args } = splitCommand(alt.cmd);
    const result = await safeExecFile(bin, args, { timeout: timeoutMs });
    if (result.exitCode === 0) {
      return alt.runner;
    }
  }
  return null;
}

/**
 * Strip shell redirections like 2>&1 from command templates.
 * Returns the clean args and whether stderr should be merged.
 */
function stripRedirections(args: string[]): { cleanArgs: string[]; mergeStderr: boolean } {
  let mergeStderr = false;
  const cleanArgs = args.filter((arg) => {
    if (arg === "2>&1") {
      mergeStderr = true;
      return false;
    }
    return true;
  });
  return { cleanArgs, mergeStderr };
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
      rule: extractRuleId(obj),
    });
  }

  return findings;
}

/**
 * Extract a rule ID from various linter JSON output shapes.
 */
function extractRuleId(obj: Record<string, unknown>): string | null {
  if (obj.rule) {
    if (typeof obj.rule === "object" && obj.rule !== null) {
      return String((obj.rule as Record<string, unknown>).id ?? obj.rule);
    }
    return String(obj.rule);
  }
  if (obj.ruleId) return String(obj.ruleId);
  if (obj.code) return String(obj.code);
  return null;
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
  // Validate file path before any execution
  validateFilePath(filePath);

  const skipped: string[] = [];

  // Filter to tools with run commands
  const runnableTools = tools.filter((t) => t.run);

  // First pass: check installation, build execution list
  const toRun: Array<{
    tool: RecommendedTool;
    bin: string;
    args: string[];
    mergeStderr: boolean;
  }> = [];

  for (const tool of runnableTools) {
    const runner = await checkInstalled(tool);
    if (runner === null) {
      skipped.push(tool.name);
      continue;
    }

    let cmdTemplate = tool.run;
    if (runner !== "system") {
      const alts = expandRunnerAlternatives(cmdTemplate);
      const match = alts.find((a) => a.runner === runner);
      if (match) cmdTemplate = match.cmd;
    }

    const { bin, args: rawArgs } = splitCommand(
      cmdTemplate.replace(/\{file\}/g, filePath)
    );
    const { cleanArgs, mergeStderr } = stripRedirections(rawArgs);
    toRun.push({ tool, bin, args: cleanArgs, mergeStderr });
  }

  // Second pass: execute all linters in parallel
  const results = await Promise.allSettled(
    toRun.map(async ({ tool, bin, args, mergeStderr }) => {
      const result = await safeExecFile(bin, args, { timeout: timeoutMs });

      if (result.exitCode === -1) {
        return { tool, error: result.stderr, findings: [] as LinterFinding[] };
      }

      const stdout = mergeStderr
        ? result.stdout + "\n" + result.stderr
        : result.stdout;

      return {
        tool,
        error: null,
        findings: parseLinterOutput(tool, stdout, result.stderr),
      };
    })
  );

  // Aggregate results
  const findings: LinterFinding[] = [];
  const errors: string[] = [];

  for (const r of results) {
    if (r.status === "rejected") {
      errors.push(String(r.reason));
      continue;
    }
    if (r.value.error) {
      errors.push(`${r.value.tool.name}: ${r.value.error}`);
    } else {
      findings.push(...r.value.findings);
    }
  }

  return { findings, skipped, errors };
}
