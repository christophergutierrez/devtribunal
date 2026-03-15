Perform a comprehensive devtribunal code review of this repository.

## Steps

1. **Detect languages**: Scan the repo for source files and identify which languages are present (TypeScript, Python, Rust, Go, Java, PHP, C#, C, Dart, Lua, SQL, Protobuf).

2. **Review files**: For each detected language, find all relevant source files and call the matching `review_*` MCP tool on each file. Use these tool names:
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

3. **Check documentation**: Call `check_docs` on README and other key documentation files.

4. **Architect synthesis**: Collect all findings from step 2 and 3 as a JSON string. Call `architect` with these combined findings to identify cross-cutting concerns and systemic patterns.

5. **Manager action plan**: Call `manager` with the architect output and findings to produce a prioritized, effort-rated action plan.

6. **Present results**: Show the final action plan to the user, organized by priority and work unit.
