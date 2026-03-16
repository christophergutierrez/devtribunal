# devtribunal — Design Decisions

An MCP server where each tool is a specialist review agent. Agents deliver structured Markdown findings, not freeform chat. Multiple specialists can be composed and synthesized into a prioritized action plan.

## Core Idea

- Each MCP tool is a specialist agent with domain expertise and structured output
- The calling AI assistant (Claude Code) orchestrates which agents to invoke and synthesizes results
- Agents run external linters when available and combine linter output with LLM-driven review

## Principles

- **Separate from repomap.** Repomap is infrastructure (parse, index, serve). devtribunal is opinions (review, analyze, recommend).
- **Agents as tools, not personas.** These are callable MCP tools with structured Markdown output, not system prompt personalities.
- **Composable.** The assistant decides which specialists to call. No hardcoded orchestration.
- **Language-aware.** Agents know their language deeply — idioms, patterns, common mistakes, ecosystem tools.
- **Pragmatic output.** Findings are severity-rated, actionable, and structured. No fluff.

## Resolved Decisions

- **Implementation language:** Rust — single binary, all agents embedded at compile time via `include_str!`
- **External tools:** Agents call linters (clippy, ruff, eslint, etc.) when available; review continues without them
- **Agent definitions:** Markdown with YAML frontmatter, configurable and extensible via `.devtribunal_agents/` in repos
- **Output format:** Structured Markdown (High-Level Summary, Critical Issues, Improvements) — not JSON
- **Host-delegated LLM:** Tools return assembled prompts; the host LLM generates the review content

## Prior Art / References

- **repomap** (christophergutierrez/repomap) — AST/symbol index, potential data source
- **agency-agents** (msitarzewski/agency-agents) — prompt persona library, inspiration for agent design but different architecture (personas vs callable tools)
- **TRB review pattern** — the manual review we did on repomap v0.4.0 is essentially what this tool automates
