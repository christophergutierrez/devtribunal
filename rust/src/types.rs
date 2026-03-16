use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentRole {
    Specialist,
    Orchestrator,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecommendedTool {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub check: String,
    #[serde(default)]
    pub run: String,
    #[serde(default)]
    pub output_format: String,
    #[serde(default)]
    pub purpose: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentFrontmatter {
    pub name: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_role")]
    pub role: AgentRole,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub severity_focus: Vec<String>,
    #[serde(default)]
    pub recommended_tools: Vec<RecommendedTool>,
    #[serde(default)]
    pub source: Option<String>,
}

fn default_role() -> AgentRole {
    AgentRole::Specialist
}

#[derive(Debug, Clone)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub role: AgentRole,
    pub languages: Vec<String>,
    pub severity_focus: Vec<String>,
    pub recommended_tools: Vec<RecommendedTool>,
    pub system_prompt: String,
    pub checklist: String,
    pub output_format: String,
    pub source: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LinterFinding {
    pub tool: String,
    pub file: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub severity: String,
    pub message: String,
    pub rule: Option<String>,
}

pub struct LinterRunResult {
    pub findings: Vec<LinterFinding>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

/// Parse a single agent definition from raw markdown with YAML frontmatter.
pub fn parse_agent(file_name: &str, raw: &str) -> anyhow::Result<AgentDefinition> {
    // Split frontmatter from body on --- delimiters
    let (frontmatter_str, body) = split_frontmatter(raw)
        .ok_or_else(|| anyhow::anyhow!("No YAML frontmatter found in {file_name}"))?;

    let frontmatter: AgentFrontmatter = serde_yaml::from_str(frontmatter_str)?;

    // Derive name from frontmatter or filename
    let name = frontmatter.name.unwrap_or_else(|| {
        file_name.strip_suffix(".md").unwrap_or(file_name).to_string()
    });

    // Split body into sections using markers
    let checklist_marker = "## Checklist";
    let output_format_marker = "## Output Format";

    let checklist_idx = body.find(checklist_marker);
    let output_format_idx = body.find(output_format_marker);

    // system_prompt = everything before the first marker
    let first_marker = [checklist_idx, output_format_idx]
        .iter()
        .filter_map(|i| *i)
        .min();

    let system_prompt = match first_marker {
        Some(idx) => body[..idx].trim().to_string(),
        None => body.trim().to_string(),
    };

    // checklist = between ## Checklist and ## Output Format (or end)
    let checklist = match checklist_idx {
        Some(ci) => {
            let start = ci + checklist_marker.len();
            let end = if output_format_idx.map_or(false, |oi| oi > ci) {
                output_format_idx.unwrap()
            } else {
                body.len()
            };
            body[start..end].trim().to_string()
        }
        None => String::new(),
    };

    // output_format = everything after ## Output Format
    let output_format = match output_format_idx {
        Some(oi) => body[oi + output_format_marker.len()..].trim().to_string(),
        None => String::new(),
    };

    Ok(AgentDefinition {
        name,
        description: frontmatter.description,
        role: frontmatter.role,
        languages: frontmatter.languages,
        severity_focus: frontmatter.severity_focus,
        recommended_tools: frontmatter.recommended_tools,
        system_prompt,
        checklist,
        output_format,
        source: frontmatter.source,
    })
}

/// Split raw markdown into (frontmatter, body) on --- delimiters.
fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    // Find the closing ---
    let after_open = &trimmed[3..];
    let close_pos = after_open.find("\n---")?;
    let frontmatter = &after_open[..close_pos];
    let body = &after_open[close_pos + 4..]; // skip \n---
    Some((frontmatter, body))
}

/// Load all embedded agent definitions.
pub fn load_embedded_agents() -> HashMap<String, AgentDefinition> {
    let mut agents = HashMap::new();
    for (filename, content) in EMBEDDED_AGENTS {
        match parse_agent(filename, content) {
            Ok(agent) => {
                agents.insert(agent.name.clone(), agent);
            }
            Err(e) => {
                tracing::warn!("Failed to parse embedded agent {filename}: {e}");
            }
        }
    }
    agents
}

/// Load all embedded skill templates.
pub fn embedded_skills() -> &'static [(&'static str, &'static str)] {
    EMBEDDED_SKILLS
}

/// Load agent definitions from a directory on disk.
pub fn load_agents_from_dir(dir: &std::path::Path) -> anyhow::Result<HashMap<String, AgentDefinition>> {
    let mut agents = HashMap::new();
    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let filename = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let raw = std::fs::read_to_string(&path)?;
        match parse_agent(&filename, &raw) {
            Ok(agent) => {
                agents.insert(agent.name.clone(), agent);
            }
            Err(e) => {
                tracing::warn!("Failed to parse agent {}: {e}", path.display());
            }
        }
    }
    Ok(agents)
}

/// Walk up from a path looking for .devtribunal_agents/ directory.
pub fn resolve_agents_dir(start_path: &str, is_directory: bool) -> Option<std::path::PathBuf> {
    let start = std::path::Path::new(start_path);
    let mut dir = if is_directory {
        start.to_path_buf()
    } else {
        start.parent()?.to_path_buf()
    };

    loop {
        let candidate = dir.join(".devtribunal_agents");
        if candidate.is_dir() {
            return Some(candidate);
        }
        if !dir.pop() {
            break;
        }
    }

    None
}

// --- Embedded assets ---

pub const EMBEDDED_AGENTS: &[(&str, &str)] = &[
    ("architect.md", include_str!("../../agents/architect.md")),
    ("check_docs.md", include_str!("../../agents/check_docs.md")),
    ("check_project_docs.md", include_str!("../../agents/check_project_docs.md")),
    ("manager.md", include_str!("../../agents/manager.md")),
    ("review_c.md", include_str!("../../agents/review_c.md")),
    ("review_csharp.md", include_str!("../../agents/review_csharp.md")),
    ("review_dart.md", include_str!("../../agents/review_dart.md")),
    ("review_go.md", include_str!("../../agents/review_go.md")),
    ("review_java.md", include_str!("../../agents/review_java.md")),
    ("review_lua.md", include_str!("../../agents/review_lua.md")),
    ("review_php.md", include_str!("../../agents/review_php.md")),
    ("review_protobuf.md", include_str!("../../agents/review_protobuf.md")),
    ("review_python.md", include_str!("../../agents/review_python.md")),
    ("review_rust.md", include_str!("../../agents/review_rust.md")),
    ("review_sql.md", include_str!("../../agents/review_sql.md")),
    ("review_typescript.md", include_str!("../../agents/review_typescript.md")),
];

const EMBEDDED_SKILLS: &[(&str, &str)] = &[
    ("full.md", include_str!("../../templates/skills/full.md")),
    ("incremental-pr-ready.md", include_str!("../../templates/skills/incremental-pr-ready.md")),
    ("incremental-staged.md", include_str!("../../templates/skills/incremental-staged.md")),
    ("incremental-wip.md", include_str!("../../templates/skills/incremental-wip.md")),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_specialist_agent() {
        let raw = r#"---
name: review_test
description: "Test specialist"
role: specialist
languages:
  - typescript
severity_focus:
  - type_safety
recommended_tools:
  - name: eslint
    check: "npx eslint --version"
    run: "npx eslint --format json {file}"
    output_format: json
    purpose: "Linting"
source: devtribunal
---

You are a test reviewer.

**Constraints:**
- Be specific.

## Required Output Format

Format your review.

## Checklist

### Type Safety
- Check types

## Output Format

Output here.
"#;

        let agent = parse_agent("review_test.md", raw).unwrap();
        assert_eq!(agent.name, "review_test");
        assert_eq!(agent.description, "Test specialist");
        assert_eq!(agent.role, AgentRole::Specialist);
        assert_eq!(agent.languages, vec!["typescript"]);
        assert!(!agent.system_prompt.is_empty());
        assert!(agent.system_prompt.contains("You are a test reviewer"));
        assert!(agent.system_prompt.contains("Required Output Format"));
        assert!(agent.checklist.contains("Type Safety"));
        assert!(agent.output_format.contains("Output here"));
        assert_eq!(agent.recommended_tools.len(), 1);
        assert_eq!(agent.recommended_tools[0].name, "eslint");
    }

    #[test]
    fn test_parse_orchestrator_agent() {
        let raw = r#"---
name: architect
description: "Architect orchestrator"
role: orchestrator
languages: []
recommended_tools: []
source: devtribunal
---

You are an architect.

## Checklist

### Cross-Cutting
- Check patterns

## Output Format

Format output.
"#;

        let agent = parse_agent("architect.md", raw).unwrap();
        assert_eq!(agent.name, "architect");
        assert_eq!(agent.role, AgentRole::Orchestrator);
        assert!(agent.languages.is_empty());
        assert!(agent.system_prompt.contains("You are an architect"));
        assert!(agent.checklist.contains("Cross-Cutting"));
        assert!(agent.output_format.contains("Format output"));
    }

    #[test]
    fn test_load_embedded_agents() {
        let agents = load_embedded_agents();
        assert!(agents.len() >= 16, "Expected at least 16 embedded agents, got {}", agents.len());
        assert!(agents.contains_key("review_typescript"));
        assert!(agents.contains_key("architect"));
        assert!(agents.contains_key("manager"));
        assert!(agents.contains_key("check_docs"));
        assert!(agents.contains_key("check_project_docs"));

        // Verify roles
        assert_eq!(agents["review_typescript"].role, AgentRole::Specialist);
        assert_eq!(agents["architect"].role, AgentRole::Orchestrator);
        assert_eq!(agents["manager"].role, AgentRole::Orchestrator);
    }

    #[test]
    fn test_name_fallback_from_filename() {
        let raw = r#"---
description: "No name field"
---

Body content.
"#;
        let agent = parse_agent("my_agent.md", raw).unwrap();
        assert_eq!(agent.name, "my_agent");
    }
}
