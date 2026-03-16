import { readFile, readdir, stat } from "node:fs/promises";
import { join, basename, dirname } from "node:path";
import { z } from "zod";
import matter from "gray-matter";
import type { AgentDefinition, AgentRole, RecommendedTool } from "./types.js";

const RecommendedToolSchema = z.object({
  name: z.string().default(""),
  check: z.string().default(""),
  run: z.string().default(""),
  output_format: z.string().default(""),
  purpose: z.string().default(""),
});

const AgentFrontmatterSchema = z.object({
  name: z.string().optional(),
  description: z.string().default(""),
  role: z.enum(["specialist", "orchestrator"]).default("specialist"),
  languages: z.array(z.string()).default([]),
  severity_focus: z.array(z.string()).default([]),
  recommended_tools: z.array(RecommendedToolSchema).default([]),
});

export function parseAgent(filePath: string, raw: string): AgentDefinition {
  const { data, content } = matter(raw);

  const parsed = AgentFrontmatterSchema.safeParse(data);
  if (!parsed.success) {
    throw new Error(
      `Invalid agent definition in ${filePath}: ${parsed.error.message}`
    );
  }

  const frontmatter = parsed.data;
  const name = frontmatter.name ?? basename(filePath, ".md");
  const description = frontmatter.description;
  const role: AgentRole = frontmatter.role;
  const languages: string[] = frontmatter.languages;
  const severity_focus: string[] = frontmatter.severity_focus;
  const recommended_tools: RecommendedTool[] = frontmatter.recommended_tools;

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

  const mdFiles = entries.filter((e) => e.endsWith(".md"));
  const loaded = await Promise.all(
    mdFiles.map((entry) => loadAgent(join(agentsDir, entry)))
  );

  for (const agent of loaded) {
    agents.set(agent.name, agent);
  }

  return agents;
}
