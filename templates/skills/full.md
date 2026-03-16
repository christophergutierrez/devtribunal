Perform a comprehensive devtribunal code review of this repository.

## Steps

1. **Detect languages**: Scan the repo for source files and identify which languages are present (TypeScript, Python, Rust, Go, Java, PHP, C#, C, Dart, Lua, SQL, Protobuf).

2. **Gather structural context (if repomap is available)**: If `get_file_outline`, `find_implementations`, or `graph_query` MCP tools are available, use them before reviewing to enrich each review with codebase context:
   - `get_file_outline` on each file to understand its symbol structure
   - `find_implementations` for files that define interfaces, traits, or abstract classes
   - `get_repo_outline` once to give the architect a high-level codebase overview
   Pass this context as the `context` parameter when calling review tools below.

3. **Review files**: For each detected language, find all relevant source files and call the matching `review_*` MCP tool on each file. Use these tool names:
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

5. **Architect synthesis**: Collect all Markdown findings from steps 3 and 4 into a single string. Call the `architect` orchestrator tool with these combined findings to identify cross-cutting concerns and systemic patterns. If repomap context was gathered in step 2, include the repo outline as additional context.

6. **Check project-level documentation**: Read the contents of README, CHANGELOG, architecture docs, and other project-level documentation files. Call the `check_project_docs` orchestrator tool with the architect output as `findings` and the documentation contents as `context`. This checks whether project docs are consistent with what the review revealed.

7. **Manager action plan**: Call the `manager` orchestrator tool with the architect output, the project docs audit from step 6, and the original specialist findings to produce a prioritized, effort-rated action plan.

8. **Present results**: Show the final action plan to the user, organized by priority and work unit.
