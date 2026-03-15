import { readFile, readdir, stat } from "node:fs/promises";
import { join, basename, dirname } from "node:path";
import matter from "gray-matter";
import type { AgentDefinition, AgentRole, RecommendedTool } from "./types.js";

export function parseAgent(filePath: string, raw: string): AgentDefinition {
  const { data, content } = matter(raw);

  const name = data.name ?? basename(filePath, ".md");
  const description = data.description ?? "";
  const role: AgentRole = data.role === "orchestrator" ? "orchestrator" : "specialist";
  const languages: string[] = data.languages ?? [];
  const severity_focus: string[] = data.severity_focus ?? [];

  const recommended_tools: RecommendedTool[] = (data.recommended_tools ?? []).map(
    (t: Record<string, string>) => ({
      name: t.name ?? "",
      check: t.check ?? "",
      run: t.run ?? "",
      output_format: t.output_format ?? "",
      purpose: t.purpose ?? "",
    })
  );

  // Split body into system prompt and checklist sections
  const checklistMarker = "## Checklist";
  const checklistIndex = content.indexOf(checklistMarker);

  let system_prompt: string;
  let checklist: string;

  if (checklistIndex >= 0) {
    system_prompt = content.slice(0, checklistIndex).trim();
    checklist = content.slice(checklistIndex + checklistMarker.length).trim();
  } else {
    system_prompt = content.trim();
    checklist = "";
  }

  return {
    name,
    description,
    role,
    languages,
    severity_focus,
    recommended_tools,
    system_prompt,
    checklist,
  };
}

export async function loadAgent(agentPath: string): Promise<AgentDefinition> {
  const raw = await readFile(agentPath, "utf-8");
  return parseAgent(agentPath, raw);
}

/**
 * Walk up from a file path looking for devtribunal_agents/ directory.
 * Returns the path if found, otherwise returns the built-in agents dir.
 */
export async function resolveAgentsDir(
  filePath: string,
  builtinDir: string,
  kind: "file" | "directory" = "file"
): Promise<string> {
  let dir = kind === "directory" ? filePath : dirname(filePath);
  const root = "/";

  while (dir !== root) {
    const candidate = join(dir, "devtribunal_agents");
    try {
      const s = await stat(candidate);
      if (s.isDirectory()) return candidate;
    } catch {
      // not found, keep walking
    }
    dir = dirname(dir);
  }

  return builtinDir;
}

export async function loadAllAgents(
  agentsDir: string
): Promise<Map<string, AgentDefinition>> {
  const agents = new Map<string, AgentDefinition>();

  let entries: string[];
  try {
    entries = await readdir(agentsDir);
  } catch {
    return agents;
  }

  for (const entry of entries) {
    if (!entry.endsWith(".md")) continue;
    const agentPath = join(agentsDir, entry);
    const agent = await loadAgent(agentPath);
    agents.set(agent.name, agent);
  }

  return agents;
}
