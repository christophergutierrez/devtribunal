import { z } from "zod";
import type { AgentDefinition } from "../types.js";

const OrchestrateInputSchema = z.object({
  findings: z
    .string()
    .describe("JSON string of review findings from specialist agents"),
  context: z
    .string()
    .optional()
    .describe("Additional context about the review scope or priorities"),
});

export type OrchestrateInput = z.infer<typeof OrchestrateInputSchema>;

export { OrchestrateInputSchema };

export const orchestrateInputJsonSchema = {
  type: "object" as const,
  properties: {
    findings: {
      type: "string" as const,
      description: "JSON string of review findings from specialist agents",
    },
    context: {
      type: "string" as const,
      description:
        "Additional context about the review scope or priorities",
    },
  },
  required: ["findings"],
};

export function buildOrchestratePrompt(
  agent: AgentDefinition,
  findings: string,
  context?: string
): string {
  const parts: string[] = [];

  parts.push(agent.system_prompt);

  if (agent.checklist) {
    parts.push("\n## Process\n");
    parts.push(agent.checklist);
  }

  parts.push("\n## Specialist Findings\n");
  parts.push("```json");
  parts.push(findings);
  parts.push("```");

  if (context) {
    parts.push("\n## Additional Context\n");
    parts.push(context);
  }

  if (agent.output_format) {
    parts.push("\n## Required Output Format\n");
    parts.push(agent.output_format);
  }

  return parts.join("\n");
}

export async function executeOrchestrate(
  agent: AgentDefinition,
  input: OrchestrateInput
): Promise<{ content: string; isError: boolean }> {
  try {
    JSON.parse(input.findings);
  } catch (err) {
    return {
      content: `Invalid findings JSON: ${err instanceof Error ? err.message : String(err)}. Expected a JSON string from specialist agent output.`,
      isError: true,
    };
  }

  const prompt = buildOrchestratePrompt(agent, input.findings, input.context);
  return { content: prompt, isError: false };
}
