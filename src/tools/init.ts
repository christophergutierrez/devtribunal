import { readFile, readdir, mkdir, writeFile, stat } from "node:fs/promises";
import { join, extname } from "node:path";
import { z } from "zod";
import matter from "gray-matter";

export const InitInputSchema = z.object({
  repo_path: z.string().describe("Absolute path to the target repository"),
  languages: z
    .array(z.string())
    .optional()
    .describe("Languages to initialize agents for (auto-detected if omitted)"),
});

export type InitInput = z.infer<typeof InitInputSchema>;

export const initInputJsonSchema = {
  type: "object" as const,
  properties: {
    repo_path: {
      type: "string" as const,
      description: "Absolute path to the target repository",
    },
    languages: {
      type: "array" as const,
      items: { type: "string" as const },
      description:
        "Languages to initialize agents for (auto-detected if omitted)",
    },
  },
  required: ["repo_path"],
};

const EXTENSION_TO_LANGUAGE: Record<string, string> = {
  ".ts": "typescript",
  ".tsx": "typescript",
  ".js": "javascript",
  ".jsx": "javascript",
  ".py": "python",
  ".rs": "rust",
  ".go": "go",
  ".java": "java",
  ".php": "php",
  ".cs": "csharp",
  ".c": "c",
  ".h": "c",
  ".dart": "dart",
  ".lua": "lua",
  ".sql": "sql",
  ".proto": "protobuf",
};

const GITIGNORE_ENTRIES = [
  ".devtribunal_agents/",
  ".claude/commands/dt/",
];

async function detectLanguages(repoPath: string): Promise<Set<string>> {
  const languages = new Set<string>();
  const dirsToScan = [
    repoPath,
    join(repoPath, "src"),
    join(repoPath, "lib"),
    join(repoPath, "app"),
    join(repoPath, "cmd"),
    join(repoPath, "internal"),
    join(repoPath, "packages"),
    join(repoPath, "test"),
    join(repoPath, "tests"),
  ];

  for (const dir of dirsToScan) {
    let entries: string[];
    try {
      entries = await readdir(dir);
    } catch {
      continue;
    }
    for (const entry of entries) {
      const ext = extname(entry);
      const lang = EXTENSION_TO_LANGUAGE[ext];
      if (lang) languages.add(lang);
    }
  }

  return languages;
}

async function fileExists(path: string): Promise<boolean> {
  try {
    await stat(path);
    return true;
  } catch {
    return false;
  }
}

/**
 * Ensure .gitignore in the target repo contains entries for devtribunal paths.
 * Returns the list of entries that were added (empty if all already present).
 */
async function ensureGitignore(
  repoPath: string,
  entries: string[]
): Promise<string[]> {
  const gitignorePath = join(repoPath, ".gitignore");

  let existing = "";
  try {
    existing = await readFile(gitignorePath, "utf-8");
  } catch {
    // No .gitignore yet — will create one
  }

  const existingLines = existing.split("\n").map((l) => l.trim());
  const toAdd = entries.filter((entry) => !existingLines.includes(entry));

  if (toAdd.length === 0) return [];

  const addition =
    (existing && !existing.endsWith("\n") ? "\n" : "") +
    "\n# devtribunal (remove these lines to version-control agents and skills)\n" +
    toAdd.join("\n") +
    "\n";

  await writeFile(gitignorePath, existing + addition, "utf-8");
  return toAdd;
}

interface ScaffoldResult {
  results: string[];
  written: number;
  skipped: number;
}

async function scaffoldSkills(
  repoPath: string,
  skillTemplatesDir: string
): Promise<ScaffoldResult> {
  const targetDir = join(repoPath, ".claude", "commands", "dt");
  const results: string[] = [];
  let written = 0;
  let skipped = 0;

  let templateFiles: string[];
  try {
    templateFiles = (await readdir(skillTemplatesDir)).filter((f) =>
      f.endsWith(".md")
    );
  } catch {
    return { results: ["  No skill templates found"], written: 0, skipped: 0 };
  }

  await mkdir(targetDir, { recursive: true });

  for (const filename of templateFiles) {
    const templateContent = await readFile(
      join(skillTemplatesDir, filename),
      "utf-8"
    );
    const targetPath = join(targetDir, filename);

    if (await fileExists(targetPath)) {
      const existingContent = await readFile(targetPath, "utf-8");
      if (existingContent.trim() === templateContent.trim()) {
        results.push(`  SKIPPED ${filename} (already current)`);
        skipped++;
        continue;
      }
      results.push(`  SKIPPED ${filename} (modified by user — won't overwrite)`);
      skipped++;
      continue;
    }

    await writeFile(targetPath, templateContent, "utf-8");
    results.push(`  WROTE   ${filename}`);
    written++;
  }

  return { results, written, skipped };
}

export async function executeInit(
  input: InitInput,
  builtinAgentsDir: string
): Promise<{ content: string; isError: boolean }> {
  const targetDir = join(input.repo_path, ".devtribunal_agents");

  // Detect or use provided languages
  const languages = input.languages
    ? new Set(input.languages)
    : await detectLanguages(input.repo_path);

  if (languages.size === 0) {
    return {
      content:
        "No supported languages detected in this repository. " +
        "You can specify languages explicitly: { languages: [\"typescript\", \"python\"] }",
      isError: false,
    };
  }

  // Load built-in agents and filter to relevant languages
  let builtinFiles: string[];
  try {
    builtinFiles = (await readdir(builtinAgentsDir)).filter((f) =>
      f.endsWith(".md")
    );
  } catch {
    return {
      content: "Cannot read built-in agents directory: " + builtinAgentsDir,
      isError: true,
    };
  }

  // Match agents: language-specific agents that match detected languages,
  // plus all language-agnostic agents (orchestrators, check_docs)
  const relevantAgents: Array<{ filename: string; raw: string }> = [];
  for (const filename of builtinFiles) {
    const raw = await readFile(join(builtinAgentsDir, filename), "utf-8");
    const { data } = matter(raw);
    const agentLangs: string[] = data.languages ?? [];
    const isLanguageAgnostic = agentLangs.length === 0;
    const matchesLanguage = agentLangs.some((l: string) => languages.has(l));
    if (isLanguageAgnostic || matchesLanguage) {
      relevantAgents.push({ filename, raw });
    }
  }

  if (relevantAgents.length === 0) {
    return {
      content: `Detected languages: ${[...languages].join(", ")}. No matching agents available yet.`,
      isError: false,
    };
  }

  // Create target directory
  await mkdir(targetDir, { recursive: true });

  const results: string[] = [];
  let written = 0;
  let skipped = 0;

  for (const { filename, raw } of relevantAgents) {
    const targetPath = join(targetDir, filename);

    if (await fileExists(targetPath)) {
      // Check if user has modified it
      const existingRaw = await readFile(targetPath, "utf-8");
      const { data: existingData } = matter(existingRaw);

      if (existingData.source === "custom") {
        results.push(`  SKIPPED ${filename} (source: custom — user-created)`);
        skipped++;
        continue;
      }

      if (existingRaw.trim() === raw.trim()) {
        results.push(`  SKIPPED ${filename} (already current)`);
        skipped++;
        continue;
      }

      // Content differs from built-in — user has edited it
      results.push(
        `  SKIPPED ${filename} (modified by user — won't overwrite)`
      );
      skipped++;
      continue;
    }

    await writeFile(targetPath, raw, "utf-8");
    results.push(`  WROTE   ${filename}`);
    written++;
  }

  // Scaffold skills
  const skillTemplatesDir = join(builtinAgentsDir, "..", "templates", "skills");
  const skillResult = await scaffoldSkills(input.repo_path, skillTemplatesDir);

  // Add to .gitignore if anything was written
  const totalWritten = written + skillResult.written;
  let gitignoreAdded: string[] = [];
  if (totalWritten > 0) {
    gitignoreAdded = await ensureGitignore(input.repo_path, GITIGNORE_ENTRIES);
  }

  const summary = [
    `Languages detected: ${[...languages].join(", ")}`,
    "",
    `## Agents → ${targetDir}`,
    ...results,
    `${written} written, ${skipped} skipped`,
    "",
    `## Skills → ${join(input.repo_path, ".claude", "commands", "dt")}`,
    ...skillResult.results,
    `${skillResult.written} written, ${skillResult.skipped} skipped`,
  ];

  if (gitignoreAdded.length > 0) {
    summary.push(
      "",
      `## .gitignore`,
      `  Added: ${gitignoreAdded.join(", ")}`,
      `  These paths are gitignored by default (no trace in your repo).`
    );
  }

  if (totalWritten > 0) {
    summary.push(
      "",
      "You can:",
      "  - Edit agent files to customize review criteria for your team",
      "  - Use /dt:full, /dt:incremental-pr-ready, /dt:incremental-staged, /dt:incremental-wip",
      "  - To version-control your agents and skills, remove the devtribunal lines from .gitignore",
      "  - Add source: custom to agent frontmatter for files you create from scratch"
    );
  }

  return { content: summary.join("\n"), isError: false };
}
