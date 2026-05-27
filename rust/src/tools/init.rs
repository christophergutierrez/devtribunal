use std::collections::HashSet;
use std::path::Path;

use serde_json::{json, Value};

use super::ToolResult;
use crate::lang::EXTENSION_TO_LANGUAGE;
use crate::types::{embedded_skills, parse_agent};

const GITIGNORE_ENTRIES: &[&str] = &[
    ".devtribunal_agents/",
    ".claude/commands/dt/",
    ".devtribunal/",
];

const SKIP_SCAN_DIRS: &[&str] = &[
    "node_modules", "target", "vendor", "dist", "build", "__pycache__",
    ".gradle", ".next", ".nuxt", "coverage", ".nyc_output",
];

fn detect_languages(repo_path: &Path) -> HashSet<String> {
    let mut languages = HashSet::new();

    let walker = walkdir::WalkDir::new(repo_path)
        .max_depth(3)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !name.starts_with('.') && !SKIP_SCAN_DIRS.contains(&name.as_ref())
        });

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
            let dot_ext = format!(".{ext}");
            for &(pattern, lang) in EXTENSION_TO_LANGUAGE {
                if pattern == dot_ext {
                    languages.insert(lang.to_string());
                }
            }
        }
    }

    languages
}

fn ensure_gitignore(repo_path: &Path, entries: &[&str]) -> Vec<String> {
    let gitignore_path = repo_path.join(".gitignore");
    let existing = std::fs::read_to_string(&gitignore_path).unwrap_or_default();
    let existing_lines: Vec<&str> = existing.lines().map(|l| l.trim()).collect();

    let to_add: Vec<&&str> = entries
        .iter()
        .filter(|e| !existing_lines.contains(&e.trim()))
        .collect();

    if to_add.is_empty() {
        return Vec::new();
    }

    let mut addition = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        addition.push('\n');
    }
    addition.push_str("\n# devtribunal (remove these lines to version-control agents and skills)\n");
    for entry in &to_add {
        addition.push_str(entry);
        addition.push('\n');
    }

    let new_content = format!("{existing}{addition}");
    if let Err(e) = std::fs::write(&gitignore_path, &new_content) {
        tracing::warn!("Failed to update .gitignore: {e}");
    }

    to_add.iter().map(|e| e.to_string()).collect()
}

fn ensure_mcp_json(repo_path: &Path) -> Option<String> {
    let mcp_path = repo_path.join(".mcp.json");
    let mut doc: Value = if mcp_path.exists() {
        let raw = std::fs::read_to_string(&mcp_path).unwrap_or_default();
        serde_json::from_str(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let servers = doc
        .as_object_mut()?
        .entry("mcpServers")
        .or_insert_with(|| json!({}))
        .as_object_mut()?;

    if servers.contains_key("devtribunal") {
        return None;
    }

    servers.insert(
        "devtribunal".to_string(),
        json!({
            "command": "devtribunal",
            "args": [],
            "type": "stdio"
        }),
    );

    let output = match serde_json::to_string_pretty(&doc) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("failed to serialize .mcp.json: {e}");
            return None;
        }
    };
    if let Err(e) = std::fs::write(&mcp_path, format!("{output}\n")) {
        tracing::warn!("failed to write .mcp.json: {e}");
        return None;
    }
    Some(".mcp.json".to_string())
}

// --- Managed-file provenance ---
//
// Scaffolded skills carry a trailing marker recording a hash of the content
// devtribunal wrote. On re-init this lets us tell a *pristine* file (untouched
// since we wrote it — safe to refresh) from one the user *edited* (leave alone).

const MANAGED_MARKER_PREFIX: &str = "<!-- dt:managed fnv=";
const MANAGED_MARKER_SUFFIX: &str = " -->";

fn content_fnv_hex(s: &str) -> String {
    // FNV-1a-64 — stable across releases (unlike std DefaultHasher); used only to
    // detect edits, not for any security property.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Canonical body + provenance marker, exactly as written to disk.
fn stamp_managed(content: &str) -> String {
    let body = content.trim_end();
    format!(
        "{body}\n{MANAGED_MARKER_PREFIX}{}{MANAGED_MARKER_SUFFIX}\n",
        content_fnv_hex(body)
    )
}

enum ManagedState {
    /// devtribunal wrote it and it is untouched; carries the recovered body.
    Pristine(String),
    /// Our marker is present but the body was edited since.
    UserEdited,
    /// No marker — hand-created or written before managed markers existed.
    Unmanaged,
}

fn classify_managed(on_disk: &str) -> ManagedState {
    let trimmed = on_disk.trim_end();
    if let Some((body, last_line)) = trimmed.rsplit_once('\n') {
        if let Some(rest) = last_line.strip_prefix(MANAGED_MARKER_PREFIX) {
            if let Some(stored) = rest.strip_suffix(MANAGED_MARKER_SUFFIX) {
                let body = body.trim_end();
                return if content_fnv_hex(body) == stored {
                    ManagedState::Pristine(body.to_string())
                } else {
                    ManagedState::UserEdited
                };
            }
        }
    }
    ManagedState::Unmanaged
}

#[derive(Default)]
struct SkillScaffold {
    results: Vec<String>,
    written: usize,
    updated: usize,
    skipped: usize,
    /// Files left untouched because the user edited them (had our marker, changed).
    user_edited: Vec<String>,
    /// Files left untouched because they carry no managed marker.
    unmanaged: Vec<String>,
}

fn scaffold_skills(repo_path: &Path) -> SkillScaffold {
    let target_dir = repo_path.join(".claude").join("commands").join("dt");
    let mut out = SkillScaffold::default();

    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        tracing::warn!("failed to create skills directory {}: {e}", target_dir.display());
        out.results.push("  Failed to create skills directory".to_string());
        return out;
    }

    for &(filename, content) in embedded_skills() {
        let target_path = target_dir.join(filename);
        let canonical = content.trim_end();

        if target_path.exists() {
            let existing = std::fs::read_to_string(&target_path).unwrap_or_default();
            match classify_managed(&existing) {
                ManagedState::Pristine(body) if body == canonical => {
                    out.results.push(format!("  SKIPPED {filename} (already current)"));
                    out.skipped += 1;
                }
                ManagedState::Pristine(_) => match std::fs::write(&target_path, stamp_managed(content)) {
                    Ok(_) => {
                        out.results.push(format!("  UPDATED {filename} (refreshed — was an older devtribunal version)"));
                        out.updated += 1;
                    }
                    Err(e) => out.results.push(format!("  ERROR   {filename}: {e}")),
                },
                ManagedState::UserEdited => {
                    out.results.push(format!("  SKIPPED {filename} (user-edited — left as is)"));
                    out.skipped += 1;
                    out.user_edited.push(filename.to_string());
                }
                ManagedState::Unmanaged => {
                    out.results.push(format!("  SKIPPED {filename} (no managed marker — left as is)"));
                    out.skipped += 1;
                    out.unmanaged.push(filename.to_string());
                }
            }
            continue;
        }

        match std::fs::write(&target_path, stamp_managed(content)) {
            Ok(_) => {
                out.results.push(format!("  WROTE   {filename}"));
                out.written += 1;
            }
            Err(e) => out.results.push(format!("  ERROR   {filename}: {e}")),
        }
    }

    out
}

pub fn execute_init(repo_path: &str, languages: Option<&[String]>) -> ToolResult {
    let repo = Path::new(repo_path);
    let target_dir = repo.join(".devtribunal_agents");

    // Detect or use provided languages
    let detected: HashSet<String> = match languages {
        Some(langs) => langs.iter().cloned().collect(),
        None => detect_languages(repo),
    };

    if detected.is_empty() {
        return ToolResult {
            content: "No supported languages detected in this repository. \
                You can specify languages explicitly: { languages: [\"typescript\", \"python\"] }"
                .to_string(),
            is_error: false,
        };
    }

    // Filter embedded agents to relevant ones
    let mut relevant: Vec<(&str, &str)> = Vec::new();
    for &(filename, raw) in crate::types::EMBEDDED_AGENTS {
        if let Ok(agent) = parse_agent(filename, raw) {
            let is_language_agnostic = agent.languages.is_empty();
            let matches_language = agent.languages.iter().any(|l| detected.contains(l));
            if is_language_agnostic || matches_language {
                relevant.push((filename, raw));
            }
        }
    }

    if relevant.is_empty() {
        return ToolResult {
            content: format!(
                "Detected languages: {}. No matching agents available yet.",
                detected.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
            is_error: false,
        };
    }

    // Create target directory
    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        return ToolResult {
            content: format!("Cannot create directory {}: {e}", target_dir.display()),
            is_error: true,
        };
    }

    let mut results = Vec::new();
    let mut written = 0;
    let mut skipped = 0;

    for (filename, raw) in &relevant {
        let target_path = target_dir.join(filename);

        if target_path.exists() {
            let existing_raw = match std::fs::read_to_string(&target_path) {
                Ok(s) => s,
                Err(_) => {
                    results.push(format!("  SKIPPED {filename} (cannot read existing)"));
                    skipped += 1;
                    continue;
                }
            };

            // Check source: custom
            if let Ok(agent) = parse_agent(filename, &existing_raw) {
                if agent.source.as_deref() == Some("custom") {
                    results.push(format!("  SKIPPED {filename} (source: custom — user-created)"));
                    skipped += 1;
                    continue;
                }
            }

            if existing_raw.trim() == raw.trim() {
                results.push(format!("  SKIPPED {filename} (already current)"));
                skipped += 1;
                continue;
            }

            results.push(format!("  SKIPPED {filename} (modified by user — won't overwrite)"));
            skipped += 1;
            continue;
        }

        match std::fs::write(&target_path, raw) {
            Ok(_) => {
                results.push(format!("  WROTE   {filename}"));
                written += 1;
            }
            Err(e) => {
                results.push(format!("  ERROR   {filename}: {e}"));
            }
        }
    }

    // Scaffold skills
    let skills = scaffold_skills(repo);

    // Ensure .mcp.json has devtribunal entry
    let mcp_json_added = ensure_mcp_json(repo);

    // Update .gitignore if anything was written
    let total_written = written + skills.written;
    let mut gitignore_added = Vec::new();
    if total_written > 0 {
        gitignore_added = ensure_gitignore(repo, GITIGNORE_ENTRIES);
    }

    let mut langs_sorted: Vec<_> = detected.into_iter().collect();
    langs_sorted.sort();

    let mut summary = vec![
        format!("Languages detected: {}", langs_sorted.join(", ")),
        String::new(),
        format!("## Agents → {}", target_dir.display()),
    ];
    summary.extend(results);
    summary.push(format!("{written} written, {skipped} skipped"));
    summary.push(String::new());
    summary.push(format!(
        "## Skills → {}",
        repo.join(".claude").join("commands").join("dt").display()
    ));
    summary.extend(skills.results);
    summary.push(format!(
        "{} written, {} updated, {} skipped",
        skills.written, skills.updated, skills.skipped
    ));

    if !skills.user_edited.is_empty() || !skills.unmanaged.is_empty() {
        summary.push(String::new());
        summary.push("## ⚠ Skills left untouched (NOT refreshed)".to_string());
        if !skills.user_edited.is_empty() {
            summary.push(
                "  You customized these, so they were not overwritten. The current devtribunal \
                 templates may have changed — possibly with breaking format changes. Review the \
                 latest template and merge your edits manually:"
                    .to_string(),
            );
            for f in &skills.user_edited {
                summary.push(format!("    - {f}"));
            }
        }
        if !skills.unmanaged.is_empty() {
            summary.push(
                "  These carry no devtribunal marker (older version or hand-created), so they were \
                 not overwritten. If you have NOT customized them, delete them and re-run dt_init \
                 to adopt the latest version and enable auto-updates:"
                    .to_string(),
            );
            for f in &skills.unmanaged {
                summary.push(format!("    - {f}"));
            }
        }
    }

    if mcp_json_added.is_some() {
        summary.push(String::new());
        summary.push("## .mcp.json".to_string());
        summary.push("  Added devtribunal MCP server entry (project-scope).".to_string());
        summary.push("  Claude Code will load devtribunal only when working in this repo.".to_string());
    }

    if !gitignore_added.is_empty() {
        summary.push(String::new());
        summary.push("## .gitignore".to_string());
        summary.push(format!("  Added: {}", gitignore_added.join(", ")));
        summary.push("  These paths are gitignored by default (no trace in your repo).".to_string());
    }

    if total_written > 0 {
        summary.extend([
            String::new(),
            "You can:".to_string(),
            "  - Edit agent files to customize review criteria for your team".to_string(),
            "  - Use /dt:full, /dt:incremental-pr-ready, /dt:incremental-staged, /dt:incremental-wip".to_string(),
            "  - To version-control your agents and skills, remove the devtribunal lines from .gitignore".to_string(),
            "  - Add source: custom to agent frontmatter for files you create from scratch".to_string(),
        ]);
    }

    ToolResult {
        content: summary.join("\n"),
        is_error: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitignores_devtribunal_artifact_dir() {
        // .devtribunal/ (ephemeral run artifacts) must be gitignored by dt_init,
        // separate from .devtribunal_agents/ (config).
        assert!(GITIGNORE_ENTRIES.contains(&".devtribunal/"));
        assert!(GITIGNORE_ENTRIES.contains(&".devtribunal_agents/"));
    }

    #[test]
    fn stamp_then_classify_is_pristine() {
        let tpl = "# Skill\n\nbody line\n";
        let on_disk = stamp_managed(tpl);
        match classify_managed(&on_disk) {
            ManagedState::Pristine(body) => assert_eq!(body, tpl.trim_end()),
            _ => panic!("freshly stamped content must classify as Pristine"),
        }
    }

    #[test]
    fn edited_body_classifies_as_user_edited() {
        let on_disk = stamp_managed("original body\n");
        let edited = on_disk.replace("original body", "user changed this");
        assert!(matches!(classify_managed(&edited), ManagedState::UserEdited));
    }

    #[test]
    fn no_marker_classifies_as_unmanaged() {
        assert!(matches!(classify_managed("just a file\nno marker\n"), ManagedState::Unmanaged));
    }

    #[test]
    fn scaffold_writes_then_refreshes_pristine_and_preserves_edits() {
        let dir = std::env::temp_dir().join(format!("dt_init_skills_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // First scaffold: all files written.
        let first = scaffold_skills(&dir);
        assert!(first.written > 0);
        assert_eq!(first.skipped, 0);

        // Re-run: everything pristine + current → all skipped, nothing flagged.
        let second = scaffold_skills(&dir);
        assert_eq!(second.written, 0);
        assert!(second.user_edited.is_empty());
        assert!(second.unmanaged.is_empty());
        assert_eq!(second.skipped, embedded_skills().len());

        // Edit one managed file → it is preserved and reported as user-edited.
        let skill_dir = dir.join(".claude").join("commands").join("dt");
        let (first_name, _) = embedded_skills()[0];
        let path = skill_dir.join(first_name);
        let edited = std::fs::read_to_string(&path).unwrap().replace("devtribunal", "MYCUSTOM");
        std::fs::write(&path, &edited).unwrap();
        let third = scaffold_skills(&dir);
        assert!(third.user_edited.contains(&first_name.to_string()));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited, "user-edited file must not be overwritten");

        // A pristine-but-outdated file → refreshed (UPDATED), not flagged.
        let (second_name, second_content) = embedded_skills()[1];
        let stale = stamp_managed("# stale old template\n\nold body\n");
        std::fs::write(skill_dir.join(second_name), &stale).unwrap();
        let fourth = scaffold_skills(&dir);
        assert!(fourth.updated >= 1);
        assert_eq!(
            std::fs::read_to_string(skill_dir.join(second_name)).unwrap(),
            stamp_managed(second_content),
            "pristine outdated file must be refreshed to the current template"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
