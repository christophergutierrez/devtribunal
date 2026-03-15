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

  parts.push("\n## Required Output Format\n");

  if (agent.name === "architect") {
    parts.push(`Respond with a JSON object matching this exact schema:

\`\`\`json
{
  "agent": "architect",
  "cross_cutting": [
    {
      "theme": "Name of cross-cutting concern",
      "severity": "critical | high | medium | low",
      "related_findings": ["agent:file:line references"],
      "observation": "What pattern you see across findings",
      "recommendation": "Holistic fix addressing the root cause"
    }
  ],
  "specialist_overrides": [
    {
      "original": "agent:file:line reference",
      "action": "escalate | downgrade | dismiss",
      "reason": "Why this finding should be re-evaluated"
    }
  ],
  "summary": "High-level synthesis of code health"
}
\`\`\`

Rules:
- Return ONLY the JSON object, no surrounding text
- Cross-cutting concerns should span multiple findings or files
- Only override specialist findings when you have strong architectural reasons
- Be specific — reference actual findings, not generic advice`);
  } else if (agent.name === "manager") {
    parts.push(`Respond with a JSON object matching this exact schema:

\`\`\`json
{
  "agent": "manager",
  "action_plan": [
    {
      "priority": 1,
      "work_unit": "Short title for this work unit",
      "effort": "trivial | small | medium | large",
      "impact": "critical | high | medium | low",
      "findings_addressed": ["agent:file:line references"],
      "steps": ["Concrete step 1", "Concrete step 2"],
      "rationale": "Why this priority and grouping"
    }
  ],
  "deferred": [
    {
      "finding": "agent:file:line reference",
      "reason": "Why this can wait",
      "revisit": "When to revisit this"
    }
  ],
  "summary": "X work units, estimated total effort, recommended approach"
}
\`\`\`

Rules:
- Return ONLY the JSON object, no surrounding text
- Group related findings into logical work units
- Priority 1 is highest (do first)
- Effort ratings: trivial (<15min), small (<1hr), medium (<4hr), large (>4hr)
- Be specific in steps — actionable, not vague
- Defer low-impact findings that would slow down critical fixes`);
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
