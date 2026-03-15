import { readFile } from "node:fs/promises";
import { z } from "zod";
import type { AgentDefinition } from "../types.js";

const ReviewInputSchema = z.object({
  file_path: z.string().describe("Absolute path to the file to review"),
  context: z
    .string()
    .optional()
    .describe("Additional context about the file or review focus"),
});

export type ReviewInput = z.infer<typeof ReviewInputSchema>;

export { ReviewInputSchema };

export function buildReviewPrompt(
  agent: AgentDefinition,
  fileContent: string,
  filePath: string,
  context?: string
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

  if (context) {
    parts.push("\n## Additional Context\n");
    parts.push(context);
  }

  parts.push("\n## Required Output Format\n");
  parts.push(`Respond with a JSON object matching this exact schema:

\`\`\`json
{
  "agent": "${agent.name}",
  "file": "${filePath}",
  "findings": [
    {
      "severity": "critical | high | medium | low | info",
      "confidence": "confirmed | likely | possible",
      "location": "file_path:line_number",
      "observation": "What you found",
      "why_it_matters": "Why this is a problem",
      "recommended_fix": "How to fix it"
    }
  ],
  "summary": "X critical, Y high, Z medium findings"
}
\`\`\`

Rules:
- Return ONLY the JSON object, no surrounding text
- Every finding MUST have all six fields
- Location MUST include line number when possible
- If no issues found, return empty findings array with summary "No findings"
- Be specific in observations — reference actual code, not generic advice`);

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

  const prompt = buildReviewPrompt(
    agent,
    fileContent,
    input.file_path,
    input.context
  );

  return { content: prompt, isError: false };
}
