# devtribunal

An MCP server where each tool is a specialist review agent. Agents deliver structured findings, not freeform chat. Multiple specialists can be composed and synthesized into a prioritized action plan.

## Core Idea

- Each MCP tool is a specialist agent with a persona, domain expertise, and structured output
- Agents optionally leverage repomap (if available) for AST/symbol/graph context
- Agents work without repomap too — they fall back to reading files directly
- The calling AI assistant (Claude, etc.) orchestrates which agents to invoke and synthesizes results

## Principles

- **Separate from repomap.** Repomap is infrastructure (parse, index, serve). devtribunal is opinions (review, analyze, recommend).
- **Agents as tools, not personas.** These are callable MCP tools with structured JSON output, not system prompt personalities.
- **Composable.** The assistant decides which specialists to call. No hardcoded orchestration.
- **Language-aware.** Agents know their language deeply — idioms, patterns, common mistakes, ecosystem tools.
- **Pragmatic output.** Findings are severity-rated, confidence-labeled, and actionable. No fluff.

## Planned Agents (Initial)

| Tool Name | Specialist | Focus |
|-----------|-----------|-------|
| `review_rust` | Rust specialist | Ownership, lifetimes, idiomatic patterns, unsafe, async correctness |
| `review_python` | Python specialist | Type safety, import hygiene, async patterns, packaging |
| `review_arch` | Architect | Coupling, boundaries, dependency direction, layering |
| `check_docs` | Documentation auditor | Missing/stale docstrings, README drift, doc coverage |
| `impact_check` | Impact analyst | "What breaks if I change X" — uses dependency graph |
| `review_security` | Security specialist | OWASP top 10, injection, auth/authz, secrets exposure |

## Output Format (per agent)

```json
{
  "agent": "review_rust",
  "file": "src/main.rs",
  "findings": [
    {
      "severity": "high",
      "confidence": "confirmed",
      "location": "src/main.rs:42",
      "observation": "...",
      "why_it_matters": "...",
      "recommended_fix": "..."
    }
  ],
  "summary": "2 high, 1 medium findings"
}
```

## Architecture

- Standalone MCP server (Rust or TypeScript TBD)
- JSON-RPC 2.0 / stdio (same protocol as repomap)
- Agent definitions stored as structured config (not just prompts)
- Optional repomap integration: if repomap MCP is available, agents call its tools for symbol/graph data

## Prior Art / References

- **repomap** (christophergutierrez/repomap) — AST/symbol index, potential data source
- **agency-agents** (msitarzewski/agency-agents) — prompt persona library, inspiration for agent design but different architecture (personas vs callable tools)
- **TRB review pattern** — the manual review we did on repomap v0.4.0 is essentially what this tool automates

## Open Questions

- Rust or TypeScript for the MCP server?
- How do agents call repomap? (MCP client inside MCP server, or shared SQLite access?)
- Should agents call external tools (clippy, ruff, mypy) or purely analyze code themselves?
- How to handle agent definitions — hardcoded, or configurable/extensible?
- Pricing/token implications — each agent call is an LLM invocation, could be expensive
