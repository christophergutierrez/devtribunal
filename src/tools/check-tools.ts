import { exec } from "node:child_process";
import { z } from "zod";
import type { AgentDefinition, RecommendedTool } from "../types.js";

export const CheckToolsInputSchema = z.object({
  repo_path: z
    .string()
    .optional()
    .describe("Absolute path to the repo (uses devtribunal_agents/ if present)"),
});

export type CheckToolsInput = z.infer<typeof CheckToolsInputSchema>;

interface ToolCheckResult {
  agent: string;
  tool: string;
  purpose: string;
  installed: boolean;
  version?: string;
}

function checkCommand(cmd: string, timeoutMs = 5000): Promise<string | null> {
  return new Promise((resolve) => {
    exec(cmd, { timeout: timeoutMs }, (err, stdout) => {
      if (err) {
        resolve(null);
      } else {
        resolve(stdout.trim());
      }
    });
  });
}

export async function executeCheckTools(
  agents: Map<string, AgentDefinition>
): Promise<{ content: string; isError: boolean }> {
  const checks: Array<Promise<ToolCheckResult>> = [];

  for (const [, agent] of agents) {
    for (const tool of agent.recommended_tools) {
      checks.push(
        checkCommand(tool.check).then((version) => ({
          agent: agent.name,
          tool: tool.name,
          purpose: tool.purpose,
          installed: version !== null,
          version: version ?? undefined,
        }))
      );
    }
  }

  const results = await Promise.allSettled(checks);
  const toolResults: ToolCheckResult[] = results
    .filter(
      (r): r is PromiseFulfilledResult<ToolCheckResult> =>
        r.status === "fulfilled"
    )
    .map((r) => r.value);

  if (toolResults.length === 0) {
    return {
      content: "No recommended tools defined in loaded agents.",
      isError: false,
    };
  }

  const installed = toolResults.filter((r) => r.installed);
  const missing = toolResults.filter((r) => !r.installed);

  const lines: string[] = [
    `Tool availability: ${installed.length} of ${toolResults.length} recommended tools found`,
    "",
  ];

  if (installed.length > 0) {
    lines.push("Installed:");
    for (const r of installed) {
      lines.push(
        `  ✓ ${r.tool} (${r.purpose})${r.version ? " — " + r.version : ""}`
      );
    }
  }

  if (missing.length > 0) {
    lines.push("");
    lines.push("Missing (optional but recommended):");
    for (const r of missing) {
      lines.push(`  ✗ ${r.tool} — ${r.purpose}`);
    }
    lines.push(
      "",
      "Reviews work without these tools, but findings will be more comprehensive with them."
    );
  }

  return { content: lines.join("\n"), isError: false };
}
