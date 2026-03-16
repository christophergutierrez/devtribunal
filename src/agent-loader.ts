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

  // Split body into system prompt, checklist, and output format sections
  // Expected order: body → ## Checklist → ## Output Format
  const checklistMarker = "## Checklist";
  const outputFormatMarker = "## Output Format";

  const checklistIndex = content.indexOf(checklistMarker);
  const outputFormatIndex = content.indexOf(outputFormatMarker);

  let system_prompt: string;
  let checklist: string;
  let output_format: string;

  // Determine the end of system_prompt (first marker found, or end of content)
  const firstMarker = [checklistIndex, outputFormatIndex]
    .filter((i) => i >= 0)
    .sort((a, b) => a - b)[0];

  if (firstMarker !== undefined) {
    system_prompt = content.slice(0, firstMarker).trim();
  } else {
    system_prompt = content.trim();
  }

  // Extract checklist (between ## Checklist and ## Output Format, or end)
  if (checklistIndex >= 0) {
    const checklistStart = checklistIndex + checklistMarker.length;
    const checklistEnd = outputFormatIndex > checklistIndex ? outputFormatIndex : content.length;
    checklist = content.slice(checklistStart, checklistEnd).trim();
  } else {
    checklist = "";
  }

  // Extract output format (everything after ## Output Format)
  if (outputFormatIndex >= 0) {
    output_format = content.slice(outputFormatIndex + outputFormatMarker.length).trim();
  } else {
    output_format = "";
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
    output_format,
  };
}

export async function loadAgent(agentPath: string): Promise<AgentDefinition> {
  const raw = await readFile(agentPath, "utf-8");
  return parseAgent(agentPath, raw);
}

/**
 * Walk up from a file path looking for .devtribunal_agents/ directory.
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
    const candidate = join(dir, ".devtribunal_agents");
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
