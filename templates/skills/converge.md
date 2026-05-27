---
description: Iterative devtribunal review — review, fix, re-review the affected scope, and converge to a deterministic PASS/FAIL verdict.
---

# /dt:converge — Iterative review-and-fix loop

Drive devtribunal as a loop: review → apply fixes → re-review only the affected scope → run tests → diff findings → verdict, repeating until the verdict passes or a budget/thrash guard halts. The MCP server stays stateless; this skill owns the loop, and YOU (the calling assistant) apply the fixes with Edit. Findings are tracked deterministically by content-based id via the `diff_findings` tool.

If `$ARGUMENTS` is provided, scope the initial review to those files/directories; otherwise review the work-in-progress diff.

## Guardrails (read first)

- **MAX_PASSES = 3** by default. Never loop unboundedly.
- **Thrash halt:** if a finding id that was previously `fixed` reappears as `regressed`, STOP and escalate to the human — do not keep fighting it.
- Apply fixes only for blocking findings (critical/high, regressions, failing tests). Defer the rest.

## Loop

1. **Initial review (pass 1).**
   - Detect languages + overlays from the changed files (see language→`review_*` and overlay tables in the other dt skills).
   - Call the matching `review_*` tools (and overlay specialists) and collect each file's `## Structured Findings` json block.
   - Optionally run `architect` then `manager`; capture the architect's `## Structured Overrides` block.
   - Merge all specialist findings into one `{ "findings": [...] }` object. Record it as `previous`. Initialize `ever_fixed = []`.

2. **Apply fixes.** Using Edit, fix the blocking findings. Keep changes minimal and localized.

3. **Re-review the affected scope only.**
   - Compute the changed set since the last pass; call `blast_radius` to get impacted files.
   - Re-review = the blast-radius set PLUS any file that still had an open finding. Do NOT re-review the whole repo.
   - Collect the new Structured Findings into a `current` `{ "findings": [...] }` object.

4. **Verify with tests.** Call `run_tests` (pass `test_command` if detection is wrong). A failing suite is blocking regardless of the diff.

5. **Diff + verdict.** Call `diff_findings` with `previous`, `current`, `previously_fixed = ever_fixed`, and the architect `overrides`. Read `verdict`, `fixed`, `new`, `regressed`.
   - Append `fixed` ids to `ever_fixed`.
   - If `regressed` contains an id already seen fixed in an earlier pass → **thrash halt** (step 7, reason: thrash).

6. **Decide.**
   - `verdict == "pass"` → done, go to step 7 (success).
   - `verdict == "fail"` and `passes < MAX_PASSES` and no thrash → set `previous = current` and loop to step 2.
   - Otherwise → step 7 (halted: budget or thrash).

7. **Write artifacts** to `.devtribunal/` (gitignored by `dt_init`):
   - `.devtribunal/verdict.json` — the final `diff_findings` json (CI-consumable).
   - `.devtribunal/review-<timestamp>.md` — the verdict line, a finding ledger table (id · status · severity · location), and a prose narrative of what each pass changed.
   Report the verdict, pass count, and artifact paths to the user. On a halt, state the reason (budget exhausted or thrash on `<id>`).
