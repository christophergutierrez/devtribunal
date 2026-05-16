Perform a comprehensive devtribunal code review of this repository.

## Steps

1. **Detect languages**: Scan the repo for source files and identify which languages are present (TypeScript, Python, Rust, Go, Java, PHP, C#, C, Dart, Lua, SQL, Protobuf).

2. **Gather structural context (if repomap is available)**: If `get_file_outline`, `find_implementations`, or `graph_query` MCP tools are available, use them before reviewing to enrich each review with codebase context:
   - `get_file_outline` on each file to understand its symbol structure
   - `find_implementations` for files that define interfaces, traits, or abstract classes
   - `get_repo_outline` once to give the architect a high-level codebase overview
   Pass this context as the `context` parameter when calling review tools below.

3. **Review files**: For each detected language, find all relevant source files and review them:
   a. **Read** the file using the Read tool
   b. **Run linters**: Call the matching `review_*` MCP tool to get linter output and file metadata
   c. **Apply the Review Instructions** (see below) to produce findings for that file

   Use these tool names for linter analysis:
   - TypeScript/JavaScript: `review_typescript`
   - Python: `review_python`
   - Rust: `review_rust`
   - Go: `review_go`
   - Java: `review_java`
   - PHP: `review_php`
   - C#: `review_csharp`
   - C: `review_c`
   - Dart: `review_dart`
   - Lua: `review_lua`
   - SQL: `review_sql`
   - Protobuf: `review_protobuf`

   Run reviews in parallel where possible. Pass absolute file paths. If $ARGUMENTS is provided, scope the review to those files or directories instead of the full repo.

4. **Check file-level documentation**: Call `check_docs` on source files with significant doc comments, docstrings, or inline documentation.

5. **Structural analysis**: Run the following tools in parallel to gather codebase-wide signals:
   - `check_tracking` with `repo_path` — git hygiene audit (tracked secrets, ignored source)
   - `check_deps` with `repo_path` — dependency vulnerability scan via OSV.dev
   - `check_patterns` with `repo_path` — cross-file patterns (cycles, dead exports, duplicated literals)
   - `check_tests` with `repo_path` and `run: true` — test adequacy analysis and test execution

6. **Architect synthesis**: Collect all Markdown findings from steps 3, 4, and 5 into a single string. Call the `architect` orchestrator tool with these combined findings to identify cross-cutting concerns and systemic patterns. If repomap context was gathered in step 2, include the repo outline as additional context.

7. **Check project-level documentation**: Read the contents of README, CHANGELOG, architecture docs, and other project-level documentation files. Call the `check_project_docs` orchestrator tool with the architect output as `findings` and the documentation contents as `context`. This checks whether project docs are consistent with what the review revealed.

8. **Manager action plan**: Call the `manager` orchestrator tool with the architect output, the project docs audit from step 7, and the original specialist findings to produce a prioritized, effort-rated action plan.

9. **Present results**: Show the final action plan to the user, organized by priority and work unit.

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
