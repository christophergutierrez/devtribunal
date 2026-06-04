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

**Expected workflow for `/dt:converge`:**

1. **Make your changes** and leave them uncommitted (modified or staged) in the working tree.
2. **Run `/dt:converge`** — it reviews the work-in-progress diff, applies fixes for blocking findings *into those same changes*, re-reviews only the affected scope, runs the tests, and renders the verdict, looping until PASS or a budget/thrash guard.
3. **Commit once the verdict is PASS** — what you commit is the reviewed-and-fixed result.

Do **not** commit before converging: bare `/dt:converge` scopes to the uncommitted WIP diff, so committed work is invisible to it (use `/dt:incremental-pr-ready` to review already-committed, unpushed work instead).

Or call tools directly:

```
review_typescript({ file_path: "/path/to/file.ts" })
blast_radius({ repo_path: "/path/to/repo", scope: "staged" })
check_deps({ repo_path: "/path/to/repo" })
check_tests({ repo_path: "/path/to/repo", run: true })
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

**18 specialist agents** covering 13+ source kinds (languages, plus path/filename-routed specialists for migrations, tests, and config):

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

**10 management tools:**
- `dt_init` — scaffolds agent definitions, skill commands, `.devtribunal.yml`, and a portable `AGENTS.md` into a target repo
- `check_tools` — checks which recommended linters are installed
- `blast_radius` — diff-aware impact analysis: changed symbols + files that depend on them
- `check_tracking` — git hygiene audit: tracked secrets/artifacts, ignored source files, with fix commands
- `check_deps` — dependency vulnerability scan via OSV.dev batch API
- `check_patterns` — cross-file structural analysis: circular deps, dead exports, duplicated literals
- `check_tests` — test adequacy detection + optional test execution with parsed results
- `run_tests` — runs the project's test command in a sandboxed shell and parses pass/fail
- `check_secrets` — gitleaks-backed scan for planted/forgotten credentials in the changeset
- `diff_findings` — given two passes of structured findings, emits fixed / persisting / new / regressed + a PASS/FAIL verdict (used by the `dt:converge` loop)

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
│  ┌──────────────┐ ┌────────────┐ ┌──────────────┐      │
│  │check_tracking│ │ check_deps │ │check_patterns│      │
│  │ git hygiene  │ │ vuln scan  │ │ cycles/dead  │      │
│  └──────┬───────┘ └─────┬──────┘ └──────┬───────┘      │
│         │                │               │              │
│  ┌──────┴────────────────┴───────────────┴───────┐      │
│  │              check_tests                      │      │
│  │  test adequacy + run tests + parse results    │      │
│  └───────────────────────┬───────────────────────┘      │
│                           ▼                             │
│  Tracking issues, CVEs, patterns, test gaps/failures    │
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

Each stage is an independent MCP tool call. In host mode (default), the server returns linter output and metadata — the host LLM generates the review content using shared instructions from the skill template. In API or local mode, the server processes reviews internally and returns finished findings.

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
  backend.rs          # Multi-backend LLM support (host/api/local)
  types.rs            # Structs, agent parsing, embedded assets
  lang.rs             # Shared language detection utilities
  runner.rs           # Package runner alternatives (bunx/pnpx/npx)
  shell.rs            # Safe process execution, path validation
  tools/
    review.rs         # Specialist review: runs linters, returns metadata
    orchestrate.rs    # Orchestrator prompt builder
    linter.rs         # Linter execution (parallel, multi-format JSON parsing)
    init.rs           # dt_init scaffolding + gitignore management
    check_tools.rs    # Tool availability checker
    blast_radius.rs   # Diff-aware impact analysis (changed symbols + dependents)
    check_tracking.rs # Git hygiene audit (secrets, artifacts, ignored source)
    check_deps.rs     # Dependency vulnerability scan (OSV.dev)
    check_patterns.rs # Cross-file patterns (cycles, dead exports, duplicates)
    check_tests.rs    # Test adequacy detection + execution

agents/               # 16 agent definitions (embedded at compile time)
templates/skills/     # 4 skill templates (embedded at compile time)
```

Key design decisions:
- **Single binary** — all agents and templates embedded via `include_str!` at compile time
- **Agents are tools, not personas** — structured Markdown output, not chat
- **Config-driven** — agent definitions are markdown with YAML frontmatter
- **Multi-backend** — host mode (default, host LLM reviews), API mode (Anthropic), or local mode (OpenAI-compatible endpoint)
- **Slim tool results** — review tools return only linter output + metadata; skill templates provide review instructions once
- **Best-effort linters** — linter failures are logged via `tracing::warn` and review continues
- **Zero trace** — `dt_init` gitignores scaffolded files by default
- **Repo overrides** — `.devtribunal_agents/` in a repo overrides built-in agents (orchestrators require `repo_path` in tool call)

## Configuration

Set `DEVTRIBUNAL_BACKEND` (and a few siblings) to pick how reviews are processed. Anything not matching a row below falls back to **host** mode.

| Mode | `DEVTRIBUNAL_BACKEND=` | When to use | Required env vars |
|------|-----------------------|-------------|--------------------|
| host (default) | unset, `host`, or anything unrecognized | The calling LLM (Claude Code, Codex, …) processes the prompt itself. No extra cost beyond your session. | — |
| anthropic | `api` | Server calls the Anthropic Messages API directly and returns finished findings. | `DEVTRIBUNAL_API_KEY`, `DEVTRIBUNAL_MODEL` (defaults to `claude-sonnet-4-20250514`) |
| openai-compatible (remote, keyed) | `openai` | Server calls any OpenAI-compatible endpoint with `Authorization: Bearer …` — OpenAI / xAI (Grok) / OpenRouter / a remote vLLM. | `DEVTRIBUNAL_API_URL`, `DEVTRIBUNAL_API_KEY`, `DEVTRIBUNAL_MODEL` |
| openai-compatible (local, keyless) | `local` | Server calls a local OpenAI-compatible endpoint — Ollama / vLLM / llama.cpp. No key header is sent. | `DEVTRIBUNAL_LOCAL_URL`, `DEVTRIBUNAL_LOCAL_MODEL` |

Missing-required-var cases fall back to host mode with a warning naming the missing var. Every review result is prefixed with the active mode (e.g. `[devtribunal · openai mode · grok-code @ api.x.ai]`) so you always know what processed your code.

## Per-agent model routing

`dt_init` writes a version-controlled `.devtribunal.yml` at your repo root. Edit it to send different specialists to different models without changing any code. Routing is **backend mode only** — in host mode the calling agent picks the model, so the file is informational.

Precedence: `routes[agent] > default > server env config`.

```yaml
# .devtribunal.yml — route two specialists to two different providers
default:
  provider: anthropic
  model: claude-sonnet-4-20250514
  key_env: ANTHROPIC_API_KEY      # NAME of an env var holding the key (not the key itself)

routes:
  review_rust:                     # one specialist → xAI Grok (keyed remote)
    provider: openai
    model: grok-code
    url: https://api.x.ai/v1
    key_env: XAI_API_KEY
  review_python:                   # another specialist → local Ollama (keyless)
    provider: openai
    model: hermes-3-llama-3.1-8b
    url: http://localhost:11434/v1
  architect:                       # orchestrator → host (no server-side LLM call)
    provider: host
```

Providers: `host` (no server-side call), `anthropic` (Anthropic Messages API), `openai` (any OpenAI-compatible endpoint — keyed via `key_env` or keyless if omitted). Misconfiguration — missing `url`/`model`, or a `key_env` whose variable is unset — degrades that single route to host mode and prepends a warning to that tool's output (no silent wrong-call). A malformed file is logged and treated as absent (env config preserved).

## Host integration

**Claude Code** is the primary host: `dt_init` scaffolds `.mcp.json` and the `/dt:` slash-command skills (`dt:full`, `dt:converge`, `dt:incremental-*`) under `.claude/commands/dt/`. Nothing else to do.

**Other MCP-aware hosts (Codex, Antigravity, Grok, …)** read a repo-root **`AGENTS.md`**, which `dt_init` also scaffolds. That file is the portable form of `dt:full` + `dt:converge` expressed as plain MCP tool calls, with a per-host MCP-setup appendix (Codex `~/.codex/config.toml`, Antigravity, Grok). See `AGENTS.md` for the up-to-date snippets — not duplicated here so the two stay in sync.

**Hermes** isn't a separate host — it's a backend model. Use whichever MCP host you already use and route specialists to a local Ollama/vLLM endpoint via `.devtribunal.yml` (`provider: openai`, no `key_env` = keyless), as shown in the routing example above.
