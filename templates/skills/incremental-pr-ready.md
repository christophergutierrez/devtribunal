Perform a devtribunal code review scoped to unpushed commits (changes ready to push).

## Scope

Get the list of changed files by running:
```bash
git diff --name-only origin/main..HEAD
```

Filter to only files that still exist (exclude deletions). These are the files to review.

If no files are found, inform the user there are no unpushed changes to review.

## Steps

1. **Detect languages**: From the changed files list, identify which languages are present based on file extensions.

2. **Gather structural context (if repomap is available)**: If `get_file_outline`, `find_implementations`, or `graph_query` MCP tools are available, use them before reviewing to enrich each review with codebase context:
   - `get_file_outline` on each changed file to understand its symbol structure
   - `find_implementations` for files that define interfaces, traits, or abstract classes
   Pass this context as the `context` parameter when calling review tools below.

3. **Review files**: For each changed file, review it:
   a. **Read** the file using the Read tool
   b. **Run linters**: Call the matching `review_*` MCP tool to get linter output and file metadata
   c. **Apply the Review Instructions** (see below) to produce findings for that file

   Use these tool names for linter analysis:
   - TypeScript/JavaScript (.ts, .tsx, .js, .jsx): `review_typescript`
   - Python (.py): `review_python`
   - Rust (.rs): `review_rust`
   - Go (.go): `review_go`
   - Java (.java): `review_java`
   - PHP (.php): `review_php`
   - C# (.cs): `review_csharp`
   - C (.c, .h): `review_c`
   - Dart (.dart): `review_dart`
   - Lua (.lua): `review_lua`
   - SQL (.sql): `review_sql`
   - Protobuf (.proto): `review_protobuf`

   Run reviews in parallel where possible. Pass absolute file paths.

4. **Blast radius**: Call `blast_radius` with `repo_path` and `scope: "unpushed"` to identify which other files depend on the changed symbols. Include dependent files as context for the architect.

5. **Check documentation**: If any README or documentation files are in the changed list, call `check_docs` on them.

6. **Architect synthesis**: Collect all findings (reviews + blast radius) as a combined string. Call `architect` with these combined findings. If repomap context was gathered in step 2, include it as additional context.

7. **Manager action plan**: Call `manager` with the architect output and findings to produce a prioritized action plan.

8. **Present results**: Show the final action plan, noting this review covers unpushed commits and their blast radius.

## Review Instructions (apply to each file)

For each file you read, produce structured findings using the linter output from the review tool plus your own analysis of the file content:

**[High-Level Summary]** 2-3 sentences on the file's health and purpose.

**[Critical Issues]** Bugs, security vulnerabilities, data loss risks, concurrency hazards. Format each as:
- **Issue:** description
- **Location:** file:line or function name
- **Why:** impact/risk explanation
- **Fix:** concrete remediation

**[Improvements]** Non-blocking suggestions for better correctness, clarity, or performance. Same format.

Focus on: correctness, security, error handling, concurrency safety, resource leaks.
Skip: style/formatting issues, naming preferences, minor cosmetic concerns.

If linter findings are provided, reference them where relevant — confirm, expand on, or contextualize what the tools found.

Combine all file findings into a single document before passing to the architect.
