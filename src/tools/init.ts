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

async function detectLanguages(repoPath: string): Promise<Set<string>> {
  const languages = new Set<string>();
  const dirsToScan = [repoPath, join(repoPath, "src"), join(repoPath, "lib")];

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

export async function executeInit(
  input: InitInput,
  builtinAgentsDir: string
): Promise<{ content: string; isError: boolean }> {
  const targetDir = join(input.repo_path, "devtribunal_agents");

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

  // Match agents to detected languages
  const relevantAgents: Array<{ filename: string; raw: string }> = [];
  for (const filename of builtinFiles) {
    const raw = await readFile(join(builtinAgentsDir, filename), "utf-8");
    const { data } = matter(raw);
    const agentLangs: string[] = data.languages ?? [];
    const hasOverlap = agentLangs.some((l: string) => languages.has(l));
    if (hasOverlap) {
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

      if (existingRaw === raw) {
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

  const summary = [
    `Languages detected: ${[...languages].join(", ")}`,
    `Target: ${targetDir}`,
    "",
    ...results,
    "",
    `${written} written, ${skipped} skipped`,
  ];

  if (written > 0) {
    summary.push(
      "",
      "Agent files are now in your repo. You can:",
      "  - Edit them to customize review criteria for your team",
      "  - Commit them to version control",
      "  - Add source: custom to frontmatter for files you create from scratch"
    );
  }

  return { content: summary.join("\n"), isError: false };
}
