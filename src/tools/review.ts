import { readFile } from "node:fs/promises";
import { z } from "zod";
import type { AgentDefinition } from "../types.js";
import { runLinters, type LinterRunResult } from "./linter.js";

const ReviewInputSchema = z.object({
  file_path: z.string().describe("Absolute path to the file to review"),
  context: z
    .string()
    .optional()
    .describe("Additional context about the file or review focus"),
});

export type ReviewInput = z.infer<typeof ReviewInputSchema>;

export { ReviewInputSchema };

export const reviewInputJsonSchema = {
  type: "object" as const,
  properties: {
    file_path: {
      type: "string" as const,
      description: "Absolute path to the file to review",
    },
    context: {
      type: "string" as const,
      description: "Additional context about the file or review focus",
    },
  },
  required: ["file_path"],
};

function formatLinterFindings(result: LinterRunResult): string {
  const { findings, skipped, errors } = result;

  // Nothing to report at all
  if (findings.length === 0 && skipped.length === 0 && errors.length === 0) {
    return "";
  }

  const lines: string[] = ["\n## Linter Findings\n"];

  if (findings.length > 0) {
    for (const f of findings) {
      const location = [f.file, f.line].filter(Boolean).join(":");
      const prefix = location ? `${location} — ` : "";
      lines.push(`**${f.tool}** [${f.severity}] ${prefix}${f.message}`);
    }
  } else {
    lines.push("No issues found by linters.");
  }

  if (skipped.length > 0) {
    lines.push("");
    lines.push(`**Not installed (skipped):** ${skipped.join(", ")}`);
  }

  if (errors.length > 0) {
    lines.push("");
    lines.push(`**Errors:** ${errors.join("; ")}`);
  }

  return lines.join("\n");
}

export function buildReviewPrompt(
  agent: AgentDefinition,
  fileContent: string,
  filePath: string,
  context?: string,
  linterOutput?: string
): string {
  const parts: string[] = [];

  parts.push(agent.system_prompt);

  if (agent.checklist) {
    parts.push("\n## Review Checklist\n");
    parts.push(agent.checklist);
  }

  parts.push("\n## File Under Review\n");
  parts.push(`**Path:** \`${filePath}\`\n`);

  // Add language hint from file extension
  const ext = filePath.split(".").pop() ?? "";
  const langMap: Record<string, string> = {
    ts: "typescript",
    tsx: "typescript",
    js: "javascript",
    jsx: "javascript",
    py: "python",
    rs: "rust",
    go: "go",
    java: "java",
    php: "php",
    cs: "csharp",
    c: "c",
    h: "c",
    dart: "dart",
    lua: "lua",
    sql: "sql",
    proto: "protobuf",
  };

  parts.push("```" + (langMap[ext] ?? ""));
  parts.push(fileContent);
  parts.push("```");

  if (linterOutput) {
    parts.push(linterOutput);
  }

  if (context) {
    parts.push("\n## Additional Context\n");
    parts.push(context);
  }

  // Linter cross-reference note (only when linter output is available)
  if (linterOutput) {
    parts.push("\n## Note\n");
    parts.push("If linter findings are provided above, reference them where relevant — confirm, expand on, or contextualize what the tools found.");
  }

  return parts.join("\n");
}

export async function executeReview(
  agent: AgentDefinition,
  input: ReviewInput
): Promise<{ content: string; isError: boolean }> {
  let fileContent: string;
  try {
    fileContent = await readFile(input.file_path, "utf-8");
  } catch (err) {
    return {
      content: `Cannot read file: ${input.file_path} — ${err instanceof Error ? err.message : String(err)}. Check that the path is absolute and the file exists.`,
      isError: true,
    };
  }

  // Run linters (best-effort — don't fail the review if linters break)
  let linterOutput = "";
  try {
    const linterResult = await runLinters(input.file_path, agent.recommended_tools);
    linterOutput = formatLinterFindings(linterResult);
  } catch {
    // Linter infrastructure failed — continue without linter output
  }

  const prompt = buildReviewPrompt(
    agent,
    fileContent,
    input.file_path,
    input.context,
    linterOutput
  );

  return { content: prompt, isError: false };
}
