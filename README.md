# devtribunal

MCP server where each tool is a specialist code review agent. 13 languages, structured findings, actionable plans.

## Install

### Homebrew

```sh
brew install christophergutierrez/devtribunal/devtribunal
```

### Quick install

```sh
curl -fsSL https://raw.githubusercontent.com/christophergutierrez/devtribunal/main/install.sh | sh
```

### Cargo

```sh
cargo install --git https://github.com/christophergutierrez/devtribunal
```

### Build from source

```sh
cargo build --release
cargo install --path .
```

## Setup

### 1. Configure Claude Code

```sh
claude mcp add -s user --transport stdio devtribunal "$(which devtribunal)"
```

Start a new Claude Code session after adding. The MCP tools appear automatically.

### 2. Initialize a repo

In Claude Code, call `dt_init` with a target repo path:

```
dt_init({ repo_path: "/path/to/your/repo" })
```

This auto-detects languages and scaffolds:
- `.devtribunal_agents/` — agent definition files for detected languages
- `.claude/commands/dt/` — skill commands for Claude Code

Both paths are added to `.gitignore` by default — no trace in your repo.

**Restart your Claude Code session after running `dt_init`** — Claude Code only discovers new skill commands at startup.

### 3. Run a review

Use the scaffolded skill commands:

- `/dt:full` — comprehensive review of the entire repo (includes vuln scan, git hygiene, pattern detection)
- `/dt:incremental-staged` — review staged changes + blast radius
- `/dt:incremental-pr-ready` — review unpushed commits + blast radius
- `/dt:incremental-wip` — review all work-in-progress + blast radius
- `/dt:converge` — iterative loop: review → fix → re-review the affected scope → run tests → diff findings → **PASS/FAIL verdict**. Repeats until it passes or hits a budget/thrash guard. Writes `.devtribunal/verdict.json` (CI-consumable) and an untracked `.devtribunal/review-<timestamp>.md` prose report. `.devtribunal/` is gitignored by `dt_init`.

Verdict rule (default, tunable): PASS = no open critical/high findings + zero regressions + new-finding count below threshold.

Or call tools directly:

```
review_typescript({ file_path: "/path/to/file.ts" })
blast_radius({ repo_path: "/path/to/repo", scope: "staged" })
check_deps({ repo_path: "/path/to/repo" })
```

### CLI commands

```sh
devtribunal --version         # Version check
devtribunal list-agents       # Show all embedded agents
devtribunal check-tools       # Check which linters are installed
```

No subcommand starts the MCP server (used by Claude Code automatically).

---

## What it does

AI assistants call devtribunal's review tools via MCP and get back structured, severity-rated findings — not freeform opinions. Multiple specialists can be composed and synthesized by three orchestrator agents (Architect, Project Docs Auditor, and Manager) into prioritized action plans.

**12 specialist agents** covering 13 languages:

| Agent | Languages | Linters |
|-------|-----------|---------|
| `review_typescript` | TypeScript, JavaScript | eslint, tsc, biome |
| `review_python` | Python | mypy, ruff, pylint |
| `review_rust` | Rust | clippy, cargo-audit |
| `review_go` | Go | golangci-lint, go vet, staticcheck |
| `review_java` | Java | checkstyle, spotbugs, pmd |
| `review_php` | PHP | phpstan, psalm |
| `review_csharp` | C# | dotnet-build, roslyn-analyzers, roslynator |
| `review_c` | C | clang-tidy, cppcheck |
| `review_dart` | Dart | dart analyze |
| `review_lua` | Lua | luacheck, selene |
| `review_sql` | SQL | sqlfluff |
| `review_protobuf` | Protocol Buffers | buf lint, buf breaking |
| `review_cpp` | C++ | clang-tidy, cppcheck |
| `review_shell` | Shell (bash/sh) | shellcheck, shfmt |
| `review_frontend` | HTML, CSS, SCSS, Less | htmlhint, stylelint |
| `review_migrations` | DB migrations (overlay) | — |
| `review_tests` | Test files (overlay) | — |
| `review_config` | Dockerfile, CI, Terraform (overlay) | hadolint, actionlint, tflint |

**3 orchestrator agents:**
- `architect` — synthesizes specialist findings into cross-cutting concerns
- `check_project_docs` — audits project docs (README, CHANGELOG) against architect findings for drift
- `manager` — produces prioritized, effort-rated action plans

**1 documentation auditor:**
- `check_docs` — reviews README, docstrings, and inline comments for accuracy and staleness

**6 management tools:**
- `dt_init` — scaffolds agent definitions and skill commands into a target repo
- `check_tools` — checks which recommended linters are installed
- `blast_radius` — diff-aware impact analysis: changed symbols + files that depend on them
- `check_tracking` — git hygiene audit: tracked secrets/artifacts, ignored source files, with fix commands
- `check_deps` — dependency vulnerability scan via OSV.dev batch API
- `check_patterns` — cross-file structural analysis: circular deps, dead exports, duplicated literals

## Pipeline

The host LLM (Claude Code) orchestrates the pipeline by calling MCP tools in sequence:

```
┌─────────────────────────────────────────────────────────┐
│  1. DETECT                                              │
│  Scan repo, identify languages                          │
└────────────────────────┬────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  2. REVIEW  (parallel)                                  │
│                                                         │
│  ┌──────────────┐ ┌──────────────┐ ┌──────────────┐    │
│  │ review_ts    │ │ review_py    │ │ review_rust  │    │
│  │   + eslint   │ │   + ruff     │ │   + clippy   │    │
│  │   + biome    │ │   + mypy     │ │              │    │
│  └──────┬───────┘ └──────┬───────┘ └──────┬───────┘    │
│         │                │                │    ...      │
│         ▼                ▼                ▼             │
│  ┌─────────────────────────────────────────────────┐    │
│  │           Structured Markdown findings          │    │
│  │  [High-Level Summary]                           │    │
│  │  [Critical Issues] — Issue, Location, Why, Fix  │    │
│  │  [Improvements]    — same format                │    │
│  └─────────────────────────┬───────────────────────┘    │
│                             │                           │
│  ┌──────────────┐           │                           │
│  │ check_docs   ├───────────┤  (file-level docs)       │
│  └──────────────┘           │                           │
└─────────────────────────────┼───────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────┐
│  3. STRUCTURAL ANALYSIS  (parallel)                     │
│                                                         │
│  ┌────────────────┐ ┌──────────────┐ ┌──────────────┐  │
│  │ check_tracking │ │  check_deps  │ │check_patterns│  │
│  │  git hygiene   │ │  vuln scan   │ │ cycles/dead  │  │
│  └───────┬────────┘ └──────┬───────┘ └──────┬───────┘  │
│          └─────────────────┼────────────────┘           │
│                            ▼                            │
│  Tracking issues, CVEs, structural patterns             │
└────────────────────────────┬────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────┐
│  4. ARCHITECT                                           │
│  Synthesize specialist + structural findings into:      │
│  • Cross-cutting concerns (risk vs debt, confidence)    │
│  • Specialist overrides (escalate / downgrade / dismiss)│
└────────────────────────┬────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  5. CHECK PROJECT DOCS                                  │
│  Audit project-level docs against architect findings:   │
│  • README claims contradicted by findings               │
│  • Architecture docs that don't match actual structure  │
│  • Missing docs for architectural decisions/risks       │
└────────────────────────┬────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  6. MANAGER                                             │
│  Transform all findings into:                           │
│  • Prioritized work units with effort estimates         │
│  • Concrete steps referencing specialist fixes          │
│  • Deferred items with revisit triggers                 │
└────────────────────────┬────────────────────────────────┘
                         │
                         ▼
┌─────────────────────────────────────────────────────────┐
│  7. PRESENT                                             │
│  Action plan shown to user, organized by priority       │
└─────────────────────────────────────────────────────────┘
```

Each stage is an independent MCP tool call. The server is stateless — it builds prompts from agent definitions and passes them back to the host LLM, which generates the review content.

## Customization

### Edit agent definitions

Agent files in `.devtribunal_agents/` are markdown with YAML frontmatter. Edit them to:

- Adjust the review checklist for your team's standards
- Add or remove recommended linters
- Change severity focus areas
- Add custom review instructions in the system prompt

### Create custom agents

Drop a new `.md` file in `.devtribunal_agents/` with this frontmatter:

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

Orchestrators use the same format with `role: orchestrator` and a `## Output Format` section defining their structured Markdown output.

## Architecture

```
rust/src/
  main.rs             # CLI (clap) + entry point
  mcp.rs              # JSON-RPC 2.0 stdio server
  types.rs            # Structs, agent parsing, embedded assets
  lang.rs             # Shared language detection utilities
  runner.rs           # Package runner alternatives (bunx/pnpx/npx)
  shell.rs            # Safe process execution, path validation
  tools/
    review.rs         # Specialist review prompt builder
    orchestrate.rs    # Orchestrator prompt builder
    linter.rs         # Linter execution (parallel, multi-format JSON parsing)
    init.rs           # dt_init scaffolding + gitignore management
    check_tools.rs    # Tool availability checker
    blast_radius.rs   # Diff-aware impact analysis (changed symbols + dependents)
    check_tracking.rs # Git hygiene audit (secrets, artifacts, ignored source)
    check_deps.rs     # Dependency vulnerability scan (OSV.dev)
    check_patterns.rs # Cross-file patterns (cycles, dead exports, duplicates)

agents/               # 16 agent definitions (embedded at compile time)
templates/skills/     # 4 skill templates (embedded at compile time)
```

Key design decisions:
- **Single binary** — all agents and templates embedded via `include_str!` at compile time
- **Agents are tools, not personas** — structured Markdown output, not chat
- **Config-driven** — agent definitions are markdown with YAML frontmatter
- **Host-delegated LLM** — tools return prompts, the host does the review
- **Best-effort linters** — linter failures are silently caught, review continues
- **Zero trace** — `dt_init` gitignores scaffolded files by default
- **Repo overrides** — `.devtribunal_agents/` in a repo overrides built-in agents (orchestrators require `repo_path` in tool call)
