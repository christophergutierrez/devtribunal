# devtribunal — portable agent workflow

This file is a host-agnostic guide for any MCP-aware coding agent (Codex,
Antigravity, Grok, Claude Code, etc.) to drive devtribunal end-to-end. Claude
Code users typically run `/dt:full` and `/dt:converge`; this document is the
same workflows expressed as plain MCP tool calls so any agent can follow them.

## What devtribunal is

A Rust MCP server where each tool is a specialist code-review agent. Specialists
return structured, severity-rated findings (not chat); two orchestrators
synthesize them into a prioritized action plan. The server is stateless — your
agent owns the workflow and applies the fixes.

## Tools at a glance

**Management (deterministic Rust analysis):**

- `dt_init` — scaffold `.devtribunal_agents/`, skills, `.mcp.json`, `.gitignore`,
  the version-controlled `.devtribunal.yml` routing config, and this file.
- `check_tools` — list which external linters/formatters are installed.
- `blast_radius` — compute the files affected by a change (for re-review scope).
- `check_tracking` — detect tracked secrets/binaries that shouldn't be in git.
- `check_deps` — query OSV for known CVEs in declared dependencies.
- `check_patterns` — surface structural smells / complexity hotspots.
- `check_tests` — sanity-check the test suite (runs/parses pass-fail, finds gaps).
- `run_tests` — run the project test command and parse the result.
- `check_secrets` — scan the changeset for leaked credentials (gitleaks-based).
- `diff_findings` — given two passes of structured findings, emit
  fixed / persisting / new / regressed + a PASS/FAIL verdict.

**Specialists (`review_*`):** language-specific reviewers, e.g. `review_rust`,
`review_typescript`, `review_python`, `review_go`, `review_java`,
`review_csharp`, `review_c`, `review_cpp`, `review_php`, `review_dart`,
`review_lua`, `review_sql`, `review_protobuf`, `review_shell`,
`review_frontend`, `review_migrations`, `review_tests`, `review_config`. Plus
`check_docs` and `check_project_docs`. Specialists run linters + assemble a
structured findings prompt for an LLM (yours, in host mode; the routed backend
in backend mode).

**Orchestrators:**

- `architect` — synthesis. Sees specialist *findings*, not code.
- `manager` — turns the synthesized findings into a prioritized action plan.

## Workflow: full review (`dt:full`)

Drive devtribunal for a one-shot whole-repo review.

1. **Detect languages.** Scan the repo (or call `dt_init` once to scaffold and
   record the detected languages) and decide which `review_*` specialists apply.
2. **Optional structural context.** If you also have a code-map MCP server with
   tools like `get_file_outline`, `find_implementations`, or `graph_query`,
   collect that context first and pass it as the `context` parameter on each
   specialist call below — it makes reviews materially better.
3. **Tooling sanity.** Call `check_tools { repo_path }` once so the user knows
   which linters are installed and which findings will degrade to "no linter".
4. **Cross-cutting checks** (deterministic, no LLM):
   - `check_deps { repo_path }` — CVEs in declared deps
   - `check_secrets { repo_path }` — planted/forgotten credentials
   - `check_tracking { repo_path }` — tracked artifacts that shouldn't be
   - `check_patterns { repo_path }` — structural smells
   - `check_tests { repo_path }` — test-suite health
   - `check_docs` / `check_project_docs` — doc coverage
5. **Per-file specialist passes.** For each source file relevant to a detected
   language, call the matching `review_*` tool with `{ file_path, context? }`.
   Collect every specialist's findings block.
6. **Synthesize.** Call `architect { findings, context?, repo_path? }` with all
   specialist outputs concatenated — it returns a cross-cutting synthesis.
7. **Plan.** Call `manager { findings, context?, repo_path? }` on the
   architect's output — it returns a prioritized action plan.
8. **Present** the plan to the user; you (the agent) apply fixes with your own
   edit tool. The devtribunal server does not mutate code.

## Workflow: convergence loop (`dt:converge`)

Drive devtribunal as an iterate-until-clean loop. The server stays stateless;
the loop and the fixes live in you, the calling agent. Findings carry
content-based IDs, so `diff_findings` matches them deterministically across
passes.

```
loop:
  1. Review the current scope (initial = repo or staged diff; later = focused).
  2. Synthesize (architect → manager).
  3. Apply fixes with your edit tool.
  4. Compute blast_radius of the change; re-review = blast_radius files ∪
     files of still-open findings (not the whole repo).
  5. run_tests; if test failures emerge, treat them as regressions.
  6. diff_findings(previous, current) → fixed / persisting / new / regressed
     + PASS/FAIL verdict.
  7. If PASS, stop. If regressions or thrash (oscillating findings), halt and
     report. Otherwise repeat from 1 with the focused scope.
```

Verdict gate (default, tunable per repo via the verdict thresholds the server
exposes): PASS = no open critical/high findings + zero regressions + new
findings below threshold. Two artifacts land in `.devtribunal/` (gitignored):
`verdict.json` for CI, and `review-<timestamp>.md` for humans.

## Backend modes

By default devtribunal runs in **host mode**: every `review_*`/orchestrator
call returns a prompt for *your* host LLM to process. No server-side LLM call,
no API key needed.

In **backend mode** the server calls an LLM itself. Configure via
environment variables (`DEVTRIBUNAL_BACKEND`, `DEVTRIBUNAL_API_KEY`,
`DEVTRIBUNAL_API_URL`, `DEVTRIBUNAL_MODEL`, `DEVTRIBUNAL_LOCAL_URL`,
`DEVTRIBUNAL_LOCAL_MODEL`) and, for per-agent routing, edit `.devtribunal.yml`
at the repo root. Per-agent routing is **backend-mode only**.

---

## Appendix — MCP host setup

Add devtribunal as an MCP server in your host. The server is launched as a
stdio process named `devtribunal` (installed via Homebrew on macOS/Linux or
`cargo install --path .` locally). The shape every host needs is the same:

```
command: devtribunal
args:    []
type:    stdio
```

Each host stores that block in its own config file. The snippets below are
templates; consult your host's current docs for the exact key names.

### Codex

The Codex CLI uses `~/.codex/config.toml`. Add devtribunal under
`[mcp_servers]`:

```toml
[mcp_servers.devtribunal]
command = "devtribunal"
args = []
```

For project-scope setup, also commit `.mcp.json` (created by `dt_init`) so
collaborators get the server with no extra config.

### Antigravity

Antigravity registers MCP servers in its host config (see your installation's
current docs for the exact path). The block has the same stdio shape:

```jsonc
// MCP server entry — devtribunal as a stdio process
"devtribunal": {
  "command": "devtribunal",
  "args": [],
  "type": "stdio"
}
```

Drop it into Antigravity's MCP-servers section and reload the host.

### Grok

The xAI Grok agent CLI accepts the same stdio MCP shape; add a `devtribunal`
entry to its MCP-servers config file:

```jsonc
"devtribunal": {
  "command": "devtribunal",
  "args": [],
  "type": "stdio"
}
```

If you want a specific specialist (say `review_rust`) to run against Grok in
*backend mode* rather than letting Grok process the prompt itself, edit
`.devtribunal.yml`:

```yaml
routes:
  review_rust:
    provider: openai
    model: grok-code           # or whatever Grok model id you target
    url: https://api.x.ai/v1
    key_env: XAI_API_KEY       # export XAI_API_KEY=… in your environment
```

### Hermes

Hermes is a *backend model*, not a separate MCP host. Use whichever MCP host
you already use (Claude Code, Codex, Antigravity, …) and route specialists to
your Hermes endpoint via `.devtribunal.yml` (and/or env vars). For a local
Ollama-served Hermes:

```yaml
# .devtribunal.yml — route one specialist to local Hermes (keyless)
routes:
  review_python:
    provider: openai
    model: hermes-3-llama-3.1-8b   # whatever your Ollama tag is
    url: http://localhost:11434/v1
```

Or, to set the whole server's default backend without editing the routing
file, export:

```sh
export DEVTRIBUNAL_BACKEND=local
export DEVTRIBUNAL_LOCAL_URL=http://localhost:11434/v1
export DEVTRIBUNAL_LOCAL_MODEL=hermes-3-llama-3.1-8b
```

Both routes work with vLLM and llama.cpp as well — anything OpenAI-compatible.
