# devtribunal

MCP server where each tool is a specialist code review agent. 13 languages, structured findings, actionable plans.

## What it does

AI assistants call devtribunal's review tools via MCP and get back structured, severity-rated findings — not freeform opinions. Multiple specialists can be composed and synthesized by two orchestrator agents (Architect and Manager) into prioritized action plans.

**12 specialist agents** covering 13 languages:

| Agent | Languages | Linters |
|-------|-----------|---------|
| `review_typescript` | TypeScript, JavaScript | eslint, tsc, biome |
| `review_python` | Python | ruff, mypy, pyright |
| `review_rust` | Rust | clippy, cargo-audit |
| `review_go` | Go | golangci-lint, go vet, staticcheck |
| `review_java` | Java | checkstyle, spotbugs, pmd |
| `review_php` | PHP | phpstan, psalm |
| `review_csharp` | C# | dotnet-format, roslynator |
| `review_c` | C | clang-tidy, cppcheck |
| `review_dart` | Dart | dart analyze |
| `review_lua` | Lua | luacheck, selene |
| `review_sql` | SQL | sqlfluff |
| `review_protobuf` | Protocol Buffers | buf lint |

**2 orchestrator agents:**
- `architect` — synthesizes specialist findings into cross-cutting concerns
- `manager` — produces prioritized, effort-rated action plans

**2 management tools:**
- `dt_init` — scaffolds agent definitions and skill commands into a target repo
- `check_tools` — checks which recommended linters are installed

## Install

```bash
claude mcp add devtribunal -- npx tsx /path/to/devtribunal/src/index.ts
```

Or with bun:

```bash
claude mcp add devtribunal -- bun run /path/to/devtribunal/src/index.ts
```

### Dependencies

```bash
cd /path/to/devtribunal
bun install   # or npm install
bun run build # or npx tsc
```

## Usage

### Initialize in a repo

Call `dt_init` with a target repo path. It auto-detects languages and scaffolds the matching agent definitions and Claude Code skill commands:

```
dt_init({ repo_path: "/path/to/your/repo" })
```

This creates:
- `devtribunal_agents/` — agent definition files for detected languages
- `.claude/commands/dt/` — skill commands (full, incremental-staged, incremental-pr-ready, incremental-wip)

Both paths are added to `.gitignore` by default — no trace in your repo. To version-control your agents and skills, remove the devtribunal lines from `.gitignore`.

### Review a file

Call any specialist tool with a file path:

```
review_typescript({ file_path: "/path/to/file.ts" })
```

The tool returns a structured prompt that the host LLM uses to produce JSON findings:

```json
{
  "agent": "review_typescript",
  "file": "/path/to/file.ts",
  "findings": [
    {
      "severity": "high",
      "confidence": "confirmed",
      "location": "/path/to/file.ts:42",
      "observation": "exec() used with string interpolation",
      "why_it_matters": "Command injection vector",
      "recommended_fix": "Use execFile with argument array"
    }
  ],
  "summary": "0 critical, 1 high, 0 medium findings"
}
```

If linters are installed (eslint, ruff, clippy, etc.), their output is included in the prompt so the LLM can reference concrete tool findings.

### Synthesize findings

After reviewing multiple files, pass all findings to the orchestrators:

1. `architect` — identifies cross-cutting concerns, overrides misgraded findings
2. `manager` — groups findings into prioritized work units with effort estimates

### Skill commands

After `dt_init`, these Claude Code slash commands are available:

- `/dt:full` — comprehensive review of the entire repo
- `/dt:incremental-staged` — review staged changes (ready to commit)
- `/dt:incremental-pr-ready` — review unpushed commits (ready to push)
- `/dt:incremental-wip` — review all work-in-progress changes

## Customization

### Edit agent definitions

Agent files in `devtribunal_agents/` are markdown with YAML frontmatter. Edit them to:

- Adjust the review checklist for your team's standards
- Add or remove recommended linters
- Change severity focus areas
- Add custom review instructions in the system prompt

### Create custom agents

Drop a new `.md` file in `devtribunal_agents/` with this frontmatter:

```yaml
---
name: review_myframework
description: "Custom reviewer for MyFramework patterns"
role: specialist
languages: [typescript]
source: custom
recommended_tools: []
---

Your review instructions here...

## Checklist

- Check for MyFramework anti-patterns
- Verify lifecycle hooks are used correctly
```

Set `source: custom` to prevent `dt_init` from overwriting your file on re-run.

### Create custom orchestrators

Orchestrators use the same format with `role: orchestrator` and a `## Output Format` section defining their JSON output schema. No code changes needed.

## Architecture

```
src/
  index.ts              # Entry point, stdio transport
  server.ts             # MCP server, tool routing, agent cache
  agent-loader.ts       # Parse agent markdown, Zod validation, resolve dirs
  types.ts              # TypeScript interfaces
  runner-alternatives.ts # Cross-ecosystem tool detection (bunx/pnpx/npx)
  utils/shell.ts        # Safe execFile wrapper, path validation
  tools/
    review.ts           # Specialist review prompt builder
    orchestrate.ts      # Orchestrator prompt builder
    linter.ts           # Linter runner (parallel execution, output parsing)
    init.ts             # dt_init scaffolding + gitignore management
    check-tools.ts      # Tool availability checker

agents/                 # Built-in agent definitions (15 files)
templates/skills/       # Claude Code skill templates
```

Key design decisions:
- **Agents are tools, not personas** — structured JSON output, not chat
- **Config-driven** — agent definitions are markdown, not hardcoded
- **Host-delegated LLM** — tools return prompts, the host does the review
- **Best-effort linters** — linter failures are silently caught, review continues
- **Zero trace** — `dt_init` gitignores scaffolded files by default
