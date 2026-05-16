use std::path::Path;
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::ToolResult;

const API_TIMEOUT: Duration = Duration::from_secs(15);
const OSV_BATCH_URL: &str = "https://api.osv.dev/v1/querybatch";
const MAX_BATCH_SIZE: usize = 1000;

#[derive(Debug, Clone)]
struct Dependency {
    name: String,
    version: String,
    ecosystem: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Severity {
    Critical,
    High,
    Medium,
    Low,
    Unknown,
}

impl Severity {
    fn label(&self) -> &'static str {
        match self {
            Self::Critical => "Critical",
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
            Self::Unknown => "Unknown",
        }
    }
}

#[derive(Debug, Clone)]
struct Vulnerability {
    id: String,
    summary: String,
    severity: Severity,
    package: String,
    installed_version: String,
    fixed_version: Option<String>,
    aliases: Vec<String>,
}

struct DepsAuditResult {
    lockfiles_found: Vec<String>,
    dependencies_scanned: usize,
    vulnerabilities: Vec<Vulnerability>,
    errors: Vec<String>,
}

// --- OSV API types ---

#[derive(Serialize)]
struct OsvQuery {
    package: OsvPackage,
    version: String,
}

#[derive(Serialize)]
struct OsvPackage {
    name: String,
    ecosystem: String,
}

#[derive(Serialize)]
struct OsvBatchRequest {
    queries: Vec<OsvQuery>,
}

#[derive(Deserialize)]
struct OsvBatchResponse {
    results: Vec<OsvBatchResult>,
}

#[derive(Deserialize)]
struct OsvBatchResult {
    vulns: Option<Vec<OsvVuln>>,
}

#[derive(Deserialize)]
struct OsvVuln {
    id: String,
    summary: Option<String>,
    aliases: Option<Vec<String>>,
    severity: Option<Vec<OsvSeverity>>,
    affected: Option<Vec<OsvAffected>>,
}

#[derive(Deserialize)]
struct OsvSeverity {
    score: Option<String>,
}

#[derive(Deserialize)]
struct OsvAffected {
    ranges: Option<Vec<OsvRange>>,
}

#[derive(Deserialize)]
struct OsvRange {
    events: Option<Vec<OsvEvent>>,
}

#[derive(Deserialize)]
struct OsvEvent {
    fixed: Option<String>,
}

// --- Lockfile detection and parsing ---

fn detect_lockfiles(repo_path: &Path) -> Vec<(String, String)> {
    let candidates = [
        ("Cargo.lock", "crates.io"),
        ("package-lock.json", "npm"),
        ("yarn.lock", "npm"),
        ("pnpm-lock.yaml", "npm"),
        ("go.sum", "Go"),
        ("poetry.lock", "PyPI"),
        ("requirements.txt", "PyPI"),
        ("Pipfile.lock", "PyPI"),
    ];

    candidates.iter()
        .filter(|(name, _)| repo_path.join(name).exists())
        .map(|(name, eco)| (name.to_string(), eco.to_string()))
        .collect()
}

fn parse_cargo_lock(content: &str) -> Vec<Dependency> {
    let mut deps = Vec::new();
    let mut current_name = None;
    let mut current_version = None;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[[package]]" {
            if let (Some(name), Some(version)) = (current_name.take(), current_version.take()) {
                deps.push(Dependency { name, version, ecosystem: "crates.io".into() });
            }
            current_name = None;
            current_version = None;
        } else if let Some(rest) = trimmed.strip_prefix("name = ") {
            current_name = Some(rest.trim_matches('"').to_string());
        } else if let Some(rest) = trimmed.strip_prefix("version = ") {
            current_version = Some(rest.trim_matches('"').to_string());
        }
    }
    // Last entry
    if let (Some(name), Some(version)) = (current_name, current_version) {
        deps.push(Dependency { name, version, ecosystem: "crates.io".into() });
    }

    deps
}

fn parse_package_lock_json(content: &str) -> Vec<Dependency> {
    let parsed: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut deps = Vec::new();

    // v3 format: "packages" object
    if let Some(packages) = parsed.get("packages").and_then(|p| p.as_object()) {
        for (key, val) in packages {
            if key.is_empty() {
                continue; // skip root package
            }
            let name = key.strip_prefix("node_modules/").unwrap_or(key);
            if let Some(version) = val.get("version").and_then(|v| v.as_str()) {
                deps.push(Dependency {
                    name: name.to_string(),
                    version: version.to_string(),
                    ecosystem: "npm".into(),
                });
            }
        }
    }
    // v2 fallback: "dependencies" object
    else if let Some(dependencies) = parsed.get("dependencies").and_then(|d| d.as_object()) {
        for (name, val) in dependencies {
            if let Some(version) = val.get("version").and_then(|v| v.as_str()) {
                deps.push(Dependency {
                    name: name.to_string(),
                    version: version.to_string(),
                    ecosystem: "npm".into(),
                });
            }
        }
    }

    deps
}

fn parse_go_sum(content: &str) -> Vec<Dependency> {
    let mut seen = std::collections::HashSet::new();
    let mut deps = Vec::new();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let module = parts[0];
        let version = parts[1].split('/').next().unwrap_or(parts[1]);
        let version = version.strip_prefix('v').unwrap_or(version);

        let key = format!("{module}@{version}");
        if seen.insert(key) {
            deps.push(Dependency {
                name: module.to_string(),
                version: version.to_string(),
                ecosystem: "Go".into(),
            });
        }
    }

    deps
}

fn parse_requirements_txt(content: &str) -> Vec<Dependency> {
    let mut deps = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
            continue;
        }
        // Only handle pinned versions: package==version
        if let Some((name, version)) = trimmed.split_once("==") {
            let name = name.split('[').next().unwrap_or(name).trim();
            deps.push(Dependency {
                name: name.to_string(),
                version: version.trim().to_string(),
                ecosystem: "PyPI".into(),
            });
        }
    }

    deps
}

fn parse_lockfile(filename: &str, content: &str) -> Vec<Dependency> {
    match filename {
        "Cargo.lock" => parse_cargo_lock(content),
        "package-lock.json" => parse_package_lock_json(content),
        "go.sum" => parse_go_sum(content),
        "requirements.txt" => parse_requirements_txt(content),
        "poetry.lock" => parse_cargo_lock(content), // same [[package]] format
        _ => Vec::new(),
    }
}

// --- OSV API queries ---

fn cvss_to_severity(score_str: &str) -> Severity {
    let score: f32 = score_str.parse().unwrap_or(0.0);
    if score >= 9.0 {
        Severity::Critical
    } else if score >= 7.0 {
        Severity::High
    } else if score >= 4.0 {
        Severity::Medium
    } else if score > 0.0 {
        Severity::Low
    } else {
        Severity::Unknown
    }
}

fn extract_severity(vuln: &OsvVuln) -> Severity {
    if let Some(severities) = &vuln.severity {
        for s in severities {
            if let Some(score) = &s.score {
                // CVSS vector strings contain the score; try parsing as float first
                if let Ok(f) = score.parse::<f32>() {
                    return cvss_to_severity(&f.to_string());
                }
                // Extract score from CVSS vector (e.g., "CVSS:3.1/AV:N/.../S:U")
                // For now treat as unknown if not a plain float
            }
        }
    }
    Severity::Unknown
}

fn extract_fixed_version(vuln: &OsvVuln) -> Option<String> {
    if let Some(affected) = &vuln.affected {
        for a in affected {
            if let Some(ranges) = &a.ranges {
                for range in ranges {
                    if let Some(events) = &range.events {
                        for event in events {
                            if let Some(fixed) = &event.fixed {
                                return Some(fixed.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

async fn query_osv_batch(
    client: &Client,
    deps: &[Dependency],
) -> Result<Vec<Vulnerability>, String> {
    let mut all_vulns = Vec::new();

    for chunk in deps.chunks(MAX_BATCH_SIZE) {
        let queries: Vec<OsvQuery> = chunk.iter().map(|d| OsvQuery {
            package: OsvPackage {
                name: d.name.clone(),
                ecosystem: d.ecosystem.clone(),
            },
            version: d.version.clone(),
        }).collect();

        let request = OsvBatchRequest { queries };

        let response = client
            .post(OSV_BATCH_URL)
            .json(&request)
            .send()
            .await
            .map_err(|e| format!("OSV API request failed: {e}"))?;

        if !response.status().is_success() {
            return Err(format!("OSV API returned status {}", response.status()));
        }

        let batch: OsvBatchResponse = response
            .json()
            .await
            .map_err(|e| format!("OSV API response parse failed: {e}"))?;

        for (i, result) in batch.results.iter().enumerate() {
            if let Some(vulns) = &result.vulns {
                let dep = &chunk[i];
                for vuln in vulns {
                    all_vulns.push(Vulnerability {
                        id: vuln.id.clone(),
                        summary: vuln.summary.clone().unwrap_or_else(|| "No description available".into()),
                        severity: extract_severity(vuln),
                        package: dep.name.clone(),
                        installed_version: dep.version.clone(),
                        fixed_version: extract_fixed_version(vuln),
                        aliases: vuln.aliases.clone().unwrap_or_default(),
                    });
                }
            }
        }
    }

    Ok(all_vulns)
}

// --- Output formatting ---

fn format_deps_audit(result: &DepsAuditResult) -> String {
    let mut lines = vec![
        "# Dependency Audit".to_string(),
        String::new(),
        format!("**Lockfiles scanned:** {}", result.lockfiles_found.join(", ")),
        format!("**Total dependencies:** {}", result.dependencies_scanned),
        format!("**Vulnerabilities found:** {}", result.vulnerabilities.len()),
    ];

    if result.vulnerabilities.is_empty() && result.errors.is_empty() {
        lines.push(String::new());
        lines.push("No known vulnerabilities found.".to_string());
        return lines.join("\n");
    }

    for severity in &[Severity::Critical, Severity::High, Severity::Medium, Severity::Low, Severity::Unknown] {
        let vulns: Vec<_> = result.vulnerabilities.iter()
            .filter(|v| v.severity == *severity)
            .collect();

        if vulns.is_empty() {
            continue;
        }

        lines.push(String::new());
        lines.push(format!("## {} ({})", severity.label(), vulns.len()));
        lines.push(String::new());

        for v in &vulns {
            lines.push(format!("### {} in `{}` v{}", v.id, v.package, v.installed_version));
            if !v.aliases.is_empty() {
                lines.push(format!("- **Aliases:** {}", v.aliases.join(", ")));
            }
            lines.push(format!("- **Summary:** {}", v.summary));
            if let Some(fix) = &v.fixed_version {
                lines.push(format!("- **Fix:** Upgrade to >= {fix}"));
            } else {
                lines.push("- **Fix:** No fix version available".to_string());
            }
            lines.push(String::new());
        }
    }

    if !result.errors.is_empty() {
        lines.push("## Errors".to_string());
        for e in &result.errors {
            lines.push(format!("- {e}"));
        }
    }

    lines.join("\n")
}

// --- Main execution ---

pub async fn execute_check_deps(repo_path: &str) -> ToolResult {
    let repo = Path::new(repo_path);
    if !repo.exists() {
        return ToolResult {
            content: format!("Path does not exist: {repo_path}"),
            is_error: true,
        };
    }

    let lockfiles = detect_lockfiles(repo);
    if lockfiles.is_empty() {
        return ToolResult {
            content: "No lockfiles found. Supported: Cargo.lock, package-lock.json, yarn.lock, go.sum, poetry.lock, requirements.txt".to_string(),
            is_error: false,
        };
    }

    let mut all_deps = Vec::new();
    let mut errors = Vec::new();
    let lockfile_names: Vec<String> = lockfiles.iter().map(|(name, _)| name.clone()).collect();

    for (filename, _ecosystem) in &lockfiles {
        let path = repo.join(filename);
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                errors.push(format!("Cannot read {filename}: {e}"));
                continue;
            }
        };
        let deps = parse_lockfile(filename, &content);
        all_deps.extend(deps);
    }

    let total_deps = all_deps.len();

    // Query OSV
    let client = match Client::builder()
        .timeout(API_TIMEOUT)
        .user_agent("devtribunal")
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return ToolResult {
                content: format!("Cannot create HTTP client: {e}"),
                is_error: true,
            };
        }
    };

    let vulnerabilities = match query_osv_batch(&client, &all_deps).await {
        Ok(v) => v,
        Err(e) => {
            errors.push(e);
            Vec::new()
        }
    };

    let result = DepsAuditResult {
        lockfiles_found: lockfile_names,
        dependencies_scanned: total_deps,
        vulnerabilities,
        errors,
    };

    ToolResult {
        content: format_deps_audit(&result),
        is_error: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cargo_lock() {
        let content = r#"version = 4

[[package]]
name = "anyhow"
version = "1.0.102"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "serde"
version = "1.0.228"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;
        let deps = parse_cargo_lock(content);
        assert_eq!(deps.len(), 2);
        assert_eq!(deps[0].name, "anyhow");
        assert_eq!(deps[0].version, "1.0.102");
        assert_eq!(deps[0].ecosystem, "crates.io");
        assert_eq!(deps[1].name, "serde");
    }

    #[test]
    fn test_parse_package_lock_json_v3() {
        let content = r#"{
  "name": "my-app",
  "lockfileVersion": 3,
  "packages": {
    "": { "name": "my-app", "version": "1.0.0" },
    "node_modules/lodash": { "version": "4.17.21" },
    "node_modules/express": { "version": "4.18.2" }
  }
}"#;
        let deps = parse_package_lock_json(content);
        assert_eq!(deps.len(), 2);
        assert!(deps.iter().any(|d| d.name == "lodash" && d.version == "4.17.21"));
        assert!(deps.iter().any(|d| d.name == "express" && d.version == "4.18.2"));
    }

    #[test]
    fn test_parse_go_sum() {
        let content = "github.com/pkg/errors v0.9.1 h1:abc123=\ngithub.com/pkg/errors v0.9.1/go.mod h1:def456=\ngolang.org/x/sys v0.5.0 h1:xyz789=\n";
        let deps = parse_go_sum(content);
        assert_eq!(deps.len(), 2); // deduplicated
        assert!(deps.iter().any(|d| d.name == "github.com/pkg/errors" && d.version == "0.9.1"));
        assert!(deps.iter().any(|d| d.name == "golang.org/x/sys" && d.version == "0.5.0"));
    }

    #[test]
    fn test_parse_requirements_txt() {
        let content = "# comments\nflask==2.3.1\nrequests==2.31.0\nnumpy>=1.24.0\n-r other.txt\n";
        let deps = parse_requirements_txt(content);
        assert_eq!(deps.len(), 2); // only pinned versions
        assert!(deps.iter().any(|d| d.name == "flask" && d.version == "2.3.1"));
        assert!(deps.iter().any(|d| d.name == "requests" && d.version == "2.31.0"));
    }

    #[test]
    fn test_cvss_to_severity() {
        assert_eq!(cvss_to_severity("9.8"), Severity::Critical);
        assert_eq!(cvss_to_severity("7.5"), Severity::High);
        assert_eq!(cvss_to_severity("5.0"), Severity::Medium);
        assert_eq!(cvss_to_severity("2.0"), Severity::Low);
        assert_eq!(cvss_to_severity("0.0"), Severity::Unknown);
        assert_eq!(cvss_to_severity("invalid"), Severity::Unknown);
    }

    #[test]
    fn test_detect_lockfiles_none() {
        let tmp = std::env::temp_dir().join("devtribunal_test_empty");
        std::fs::create_dir_all(&tmp).ok();
        let result = detect_lockfiles(&tmp);
        assert!(result.is_empty());
        std::fs::remove_dir_all(&tmp).ok();
    }
}
