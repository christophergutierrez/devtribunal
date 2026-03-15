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

2. **Review files**: For each changed file, call the matching `review_*` MCP tool. Use these tool names:
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

3. **Check documentation**: If any README or documentation files are in the changed list, call `check_docs` on them.

4. **Architect synthesis**: Collect all findings as a JSON string. Call `architect` with these combined findings.

5. **Manager action plan**: Call `manager` with the architect output and findings to produce a prioritized action plan.

6. **Present results**: Show the final action plan, noting this review covers only unpushed commits.
