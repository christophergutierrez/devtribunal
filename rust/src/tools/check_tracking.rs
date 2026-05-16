use std::path::Path;
use std::time::Duration;

use ignore::gitignore::GitignoreBuilder;

use super::ToolResult;
use crate::lang::{is_source_file, SKIP_DIRS};
use crate::shell::safe_exec;

const GIT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq)]
enum TrackingCategory {
    Secret,
    BuildArtifact,
    LargeBinary,
    IdeOrOs,
}

impl TrackingCategory {
    fn label(&self) -> &'static str {
        match self {
            Self::Secret => "Secrets",
            Self::BuildArtifact => "Build Artifacts",
            Self::LargeBinary => "Large Binaries",
            Self::IdeOrOs => "IDE/OS Files",
        }
    }
}

#[derive(Debug, Clone)]
struct TrackingIssue {
    path: String,
    category: TrackingCategory,
    reason: &'static str,
}

#[derive(Debug, Clone)]
struct IgnoredSourceFile {
    path: String,
}

struct TrackingAuditResult {
    tracked_issues: Vec<TrackingIssue>,
    ignored_source: Vec<IgnoredSourceFile>,
    errors: Vec<String>,
}

const SECRET_PATTERNS: &[(&str, &str)] = &[
    (".env", "Environment file may contain secrets"),
    (".env.local", "Local environment file"),
    (".env.production", "Production environment file"),
    ("id_rsa", "SSH private key"),
    ("id_ed25519", "SSH private key"),
    ("id_ecdsa", "SSH private key"),
];

const SECRET_EXTENSIONS: &[(&str, &str)] = &[
    (".key", "Private key file"),
    (".pem", "Certificate/key file"),
    (".p12", "PKCS#12 key file"),
    (".pfx", "PKCS#12 key file"),
    (".keystore", "Java keystore"),
    (".jks", "Java keystore"),
];

const SECRET_CONTAINS: &[(&str, &str)] = &[
    ("secret", "May contain secrets"),
    ("credential", "May contain credentials"),
    (".aws/", "AWS credentials directory"),
];

const ARTIFACT_PREFIXES: &[(&str, &str)] = &[
    ("node_modules/", "npm dependencies"),
    ("target/debug/", "Rust debug build"),
    ("target/release/", "Rust release build"),
    ("dist/", "Build output"),
    ("build/", "Build output"),
    ("__pycache__/", "Python bytecode cache"),
    (".gradle/", "Gradle cache"),
];

const ARTIFACT_EXTENSIONS: &[(&str, &str)] = &[
    (".pyc", "Python bytecode"),
    (".pyo", "Python optimized bytecode"),
    (".class", "Java bytecode"),
    (".o", "Object file"),
    (".obj", "Object file"),
];

const BINARY_EXTENSIONS: &[(&str, &str)] = &[
    (".exe", "Windows executable"),
    (".dll", "Windows library"),
    (".so", "Shared library"),
    (".dylib", "macOS shared library"),
    (".a", "Static library"),
    (".zip", "Archive file"),
    (".tar.gz", "Archive file"),
    (".tar.bz2", "Archive file"),
    (".rar", "Archive file"),
    (".7z", "Archive file"),
    (".jar", "Java archive"),
    (".war", "Java web archive"),
    (".whl", "Python wheel"),
];

const IDE_OS_FILES: &[(&str, &str)] = &[
    (".DS_Store", "macOS metadata"),
    ("Thumbs.db", "Windows thumbnail cache"),
    ("desktop.ini", "Windows folder settings"),
];

const IDE_PREFIXES: &[(&str, &str)] = &[
    (".idea/", "JetBrains IDE"),
    (".vscode/settings.json", "VS Code user settings"),
    (".vscode/launch.json", "VS Code debug config"),
];

const IDE_EXTENSIONS: &[(&str, &str)] = &[
    (".swp", "Vim swap file"),
    (".swo", "Vim swap file"),
];

fn classify_tracked_file(path: &str) -> Option<TrackingIssue> {
    let filename = path.rsplit('/').next().unwrap_or(path);
    let lower_path = path.to_lowercase();

    // Exact filename matches for secrets
    for &(pattern, reason) in SECRET_PATTERNS {
        if filename == pattern {
            return Some(TrackingIssue { path: path.to_string(), category: TrackingCategory::Secret, reason });
        }
    }

    // Extension matches for secrets
    for &(ext, reason) in SECRET_EXTENSIONS {
        if filename.ends_with(ext) {
            return Some(TrackingIssue { path: path.to_string(), category: TrackingCategory::Secret, reason });
        }
    }

    // Contains matches for secrets
    for &(pattern, reason) in SECRET_CONTAINS {
        if lower_path.contains(pattern) {
            return Some(TrackingIssue { path: path.to_string(), category: TrackingCategory::Secret, reason });
        }
    }

    // Artifact prefixes
    for &(prefix, reason) in ARTIFACT_PREFIXES {
        if path.starts_with(prefix) || path.contains(&format!("/{prefix}")) {
            return Some(TrackingIssue { path: path.to_string(), category: TrackingCategory::BuildArtifact, reason });
        }
    }

    // Artifact extensions
    for &(ext, reason) in ARTIFACT_EXTENSIONS {
        if filename.ends_with(ext) {
            return Some(TrackingIssue { path: path.to_string(), category: TrackingCategory::BuildArtifact, reason });
        }
    }

    // Binary extensions
    for &(ext, reason) in BINARY_EXTENSIONS {
        if path.ends_with(ext) {
            return Some(TrackingIssue { path: path.to_string(), category: TrackingCategory::LargeBinary, reason });
        }
    }

    // IDE/OS exact files
    for &(pattern, reason) in IDE_OS_FILES {
        if filename == pattern {
            return Some(TrackingIssue { path: path.to_string(), category: TrackingCategory::IdeOrOs, reason });
        }
    }

    // IDE prefixes
    for &(prefix, reason) in IDE_PREFIXES {
        if path.starts_with(prefix) || path.contains(&format!("/{prefix}")) {
            return Some(TrackingIssue { path: path.to_string(), category: TrackingCategory::IdeOrOs, reason });
        }
    }

    // IDE extensions
    for &(ext, reason) in IDE_EXTENSIONS {
        if filename.ends_with(ext) {
            return Some(TrackingIssue { path: path.to_string(), category: TrackingCategory::IdeOrOs, reason });
        }
    }

    None
}

fn find_ignored_source(repo_path: &Path) -> Vec<IgnoredSourceFile> {
    let gitignore_path = repo_path.join(".gitignore");
    if !gitignore_path.exists() {
        return Vec::new();
    }

    let mut builder = GitignoreBuilder::new(repo_path);
    builder.add(&gitignore_path);
    let gitignore = match builder.build() {
        Ok(gi) => gi,
        Err(_) => return Vec::new(),
    };

    let mut ignored = Vec::new();

    let walker = walkdir::WalkDir::new(repo_path)
        .into_iter()
        .filter_entry(|entry| {
            let name = entry.file_name().to_string_lossy();
            !SKIP_DIRS.contains(&name.as_ref())
        });

    for entry in walker.flatten() {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        let rel_path = match path.strip_prefix(repo_path) {
            Ok(p) => p,
            Err(_) => continue,
        };

        let rel_str = rel_path.to_string_lossy().to_string();

        if !is_source_file(&rel_str) {
            continue;
        }

        // Check if this file is matched by gitignore
        let matched = gitignore.matched_path_or_any_parents(rel_path, false);
        if matched.is_ignore() {
            ignored.push(IgnoredSourceFile { path: rel_str });
        }
    }

    ignored
}

fn generate_fixes(issues: &[TrackingIssue]) -> String {
    if issues.is_empty() {
        return String::new();
    }

    let mut lines = vec!["## Suggested Fixes".to_string(), String::new(), "```sh".to_string()];

    // Group by category for gitignore additions
    let secrets: Vec<_> = issues.iter().filter(|i| i.category == TrackingCategory::Secret).collect();
    let artifacts: Vec<_> = issues.iter().filter(|i| i.category == TrackingCategory::BuildArtifact).collect();
    let binaries: Vec<_> = issues.iter().filter(|i| i.category == TrackingCategory::LargeBinary).collect();
    let ide: Vec<_> = issues.iter().filter(|i| i.category == TrackingCategory::IdeOrOs).collect();

    if !secrets.is_empty() {
        lines.push("# Remove tracked secrets".to_string());
        for issue in &secrets {
            lines.push(format!("git rm --cached '{}'", issue.path));
        }
        lines.push(String::new());
    }

    if !artifacts.is_empty() {
        lines.push("# Remove tracked build artifacts".to_string());
        for issue in &artifacts {
            if issue.path.ends_with('/') || issue.path.contains('/') {
                lines.push(format!("git rm --cached -r '{}'", issue.path));
            } else {
                lines.push(format!("git rm --cached '{}'", issue.path));
            }
        }
        lines.push(String::new());
    }

    if !binaries.is_empty() {
        lines.push("# Remove tracked binaries".to_string());
        for issue in &binaries {
            lines.push(format!("git rm --cached '{}'", issue.path));
        }
        lines.push(String::new());
    }

    if !ide.is_empty() {
        lines.push("# Remove tracked IDE/OS files".to_string());
        for issue in &ide {
            lines.push(format!("git rm --cached '{}'", issue.path));
        }
        lines.push(String::new());
    }

    // Suggest gitignore additions
    lines.push("# Add to .gitignore".to_string());
    let mut gitignore_adds: Vec<String> = Vec::new();
    for issue in issues {
        let entry = suggest_gitignore_entry(&issue.path);
        if !gitignore_adds.contains(&entry) {
            gitignore_adds.push(entry);
        }
    }
    for entry in &gitignore_adds {
        lines.push(format!("echo '{entry}' >> .gitignore"));
    }

    lines.push("```".to_string());
    lines.join("\n")
}

fn suggest_gitignore_entry(path: &str) -> String {
    let filename = path.rsplit('/').next().unwrap_or(path);

    // For known patterns, suggest the pattern rather than the exact file
    if filename == ".env" || filename.starts_with(".env.") {
        return ".env*".to_string();
    }
    if filename == ".DS_Store" {
        return ".DS_Store".to_string();
    }
    if filename == "Thumbs.db" {
        return "Thumbs.db".to_string();
    }

    // For extensions, suggest the wildcard
    if let Some(ext_pos) = filename.rfind('.') {
        let ext = &filename[ext_pos..];
        if SECRET_EXTENSIONS.iter().any(|(e, _)| *e == ext)
            || BINARY_EXTENSIONS.iter().any(|(e, _)| *e == ext)
            || ARTIFACT_EXTENSIONS.iter().any(|(e, _)| *e == ext)
            || IDE_EXTENSIONS.iter().any(|(e, _)| *e == ext)
        {
            return format!("*{ext}");
        }
    }

    // For directory-based patterns, suggest the directory
    for &(prefix, _) in ARTIFACT_PREFIXES {
        if path.starts_with(prefix) {
            return prefix.to_string();
        }
    }
    for &(prefix, _) in IDE_PREFIXES {
        if path.starts_with(prefix) {
            return prefix.trim_end_matches(|c: char| c != '/').to_string();
        }
    }

    path.to_string()
}

fn format_tracking_audit(result: &TrackingAuditResult) -> String {
    let mut lines = vec!["# Git Tracking Audit".to_string(), String::new()];

    if result.tracked_issues.is_empty() && result.ignored_source.is_empty() {
        lines.push("No tracking issues found.".to_string());
        return lines.join("\n");
    }

    if !result.tracked_issues.is_empty() {
        lines.push(format!("## Tracked Files That Shouldn't Be ({} issues)", result.tracked_issues.len()));
        lines.push(String::new());

        for category in &[TrackingCategory::Secret, TrackingCategory::BuildArtifact, TrackingCategory::LargeBinary, TrackingCategory::IdeOrOs] {
            let items: Vec<_> = result.tracked_issues.iter().filter(|i| i.category == *category).collect();
            if items.is_empty() {
                continue;
            }
            lines.push(format!("### {} ({})", category.label(), items.len()));
            for item in &items {
                lines.push(format!("- `{}` — {}", item.path, item.reason));
            }
            lines.push(String::new());
        }
    }

    if !result.ignored_source.is_empty() {
        lines.push(format!("## Ignored Source Files ({} files)", result.ignored_source.len()));
        lines.push(String::new());
        lines.push("These source files are matched by `.gitignore` patterns — verify this is intentional:".to_string());
        for item in &result.ignored_source {
            lines.push(format!("- `{}`", item.path));
        }
        lines.push(String::new());
    }

    let fixes = generate_fixes(&result.tracked_issues);
    if !fixes.is_empty() {
        lines.push(fixes);
    }

    if !result.errors.is_empty() {
        lines.push(String::new());
        lines.push("## Warnings".to_string());
        for e in &result.errors {
            lines.push(format!("- {e}"));
        }
    }

    lines.join("\n")
}

pub async fn execute_check_tracking(repo_path: &str) -> ToolResult {
    let repo = Path::new(repo_path);
    if !repo.join(".git").exists() {
        return ToolResult {
            content: format!("Not a git repository: {repo_path}"),
            is_error: true,
        };
    }

    let mut errors = Vec::new();

    // Get tracked files
    let result = safe_exec("git", &[
        "-C".to_string(), repo_path.to_string(),
        "ls-files".to_string(),
    ], GIT_TIMEOUT).await;

    if result.exit_code != 0 {
        return ToolResult {
            content: format!("git ls-files failed: {}", result.stderr.trim()),
            is_error: true,
        };
    }

    // Classify tracked files
    let tracked_issues: Vec<TrackingIssue> = result.stdout.lines()
        .filter_map(|line| classify_tracked_file(line.trim()))
        .collect();

    // Find ignored source files
    let ignored_source = find_ignored_source(repo);
    if ignored_source.len() > 100 {
        errors.push(format!("Found {} ignored source files — showing first 50", ignored_source.len()));
    }

    let ignored_source_capped: Vec<IgnoredSourceFile> = ignored_source.into_iter().take(50).collect();

    let audit_result = TrackingAuditResult {
        tracked_issues,
        ignored_source: ignored_source_capped,
        errors,
    };

    ToolResult {
        content: format_tracking_audit(&audit_result),
        is_error: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_secrets() {
        assert_eq!(classify_tracked_file(".env").unwrap().category, TrackingCategory::Secret);
        assert_eq!(classify_tracked_file("config/api.key").unwrap().category, TrackingCategory::Secret);
        assert_eq!(classify_tracked_file("certs/server.pem").unwrap().category, TrackingCategory::Secret);
        assert_eq!(classify_tracked_file("id_rsa").unwrap().category, TrackingCategory::Secret);
        assert_eq!(classify_tracked_file("path/with/secret/in/it").unwrap().category, TrackingCategory::Secret);
    }

    #[test]
    fn test_classify_artifacts() {
        assert_eq!(classify_tracked_file("node_modules/lodash/index.js").unwrap().category, TrackingCategory::BuildArtifact);
        assert_eq!(classify_tracked_file("src/main.pyc").unwrap().category, TrackingCategory::BuildArtifact);
        assert_eq!(classify_tracked_file("target/debug/binary").unwrap().category, TrackingCategory::BuildArtifact);
    }

    #[test]
    fn test_classify_binaries() {
        assert_eq!(classify_tracked_file("lib/native.so").unwrap().category, TrackingCategory::LargeBinary);
        assert_eq!(classify_tracked_file("assets/archive.zip").unwrap().category, TrackingCategory::LargeBinary);
    }

    #[test]
    fn test_classify_ide() {
        assert_eq!(classify_tracked_file(".DS_Store").unwrap().category, TrackingCategory::IdeOrOs);
        assert_eq!(classify_tracked_file(".idea/workspace.xml").unwrap().category, TrackingCategory::IdeOrOs);
        assert_eq!(classify_tracked_file("src/.main.rs.swp").unwrap().category, TrackingCategory::IdeOrOs);
    }

    #[test]
    fn test_classify_clean() {
        assert!(classify_tracked_file("src/main.rs").is_none());
        assert!(classify_tracked_file("README.md").is_none());
        assert!(classify_tracked_file("Cargo.toml").is_none());
        assert!(classify_tracked_file("tests/integration_test.py").is_none());
    }

    #[test]
    fn test_suggest_gitignore_entry() {
        assert_eq!(suggest_gitignore_entry(".env"), ".env*");
        assert_eq!(suggest_gitignore_entry(".env.local"), ".env*");
        assert_eq!(suggest_gitignore_entry(".DS_Store"), ".DS_Store");
        assert_eq!(suggest_gitignore_entry("cert.pem"), "*.pem");
        assert_eq!(suggest_gitignore_entry("lib.so"), "*.so");
    }
}
