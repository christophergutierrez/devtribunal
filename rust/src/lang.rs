pub const EXTENSION_TO_LANGUAGE: &[(&str, &str)] = &[
    (".ts", "typescript"),
    (".tsx", "typescript"),
    (".js", "javascript"),
    (".jsx", "javascript"),
    (".py", "python"),
    (".rs", "rust"),
    (".go", "go"),
    (".java", "java"),
    (".php", "php"),
    (".cs", "csharp"),
    (".c", "c"),
    (".h", "c"),
    (".dart", "dart"),
    (".lua", "lua"),
    (".sql", "sql"),
    (".proto", "protobuf"),
];

pub const SOURCE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "py", "rs", "go", "java", "php", "cs", "c", "h", "dart", "lua",
];

pub const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "__pycache__",
    ".gradle",
    "vendor",
    ".next",
    ".nuxt",
    "coverage",
    ".nyc_output",
];

pub fn language_for_path(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?;
    let dot_ext = format!(".{ext}");
    EXTENSION_TO_LANGUAGE
        .iter()
        .find(|(pattern, _)| *pattern == dot_ext.as_str())
        .map(|(_, lang)| *lang)
}

pub fn is_source_file(path: &str) -> bool {
    path.rsplit('.')
        .next()
        .map(|ext| SOURCE_EXTENSIONS.contains(&ext))
        .unwrap_or(false)
}

/// Overlay specialists that apply to a file IN ADDITION to its language specialist,
/// based on filename/path conventions. A `foo_test.go` is both Go and a test, so this
/// returns extra agents (tests/migrations/config) rather than replacing the language.
/// Returns de-duplicated results in a stable order.
#[allow(dead_code)]
pub fn overlay_languages_for_path(path: &str) -> Vec<&'static str> {
    let norm = path.replace('\\', "/");
    let lower = norm.to_lowercase();
    let lower_name = lower.rsplit('/').next().unwrap_or(&lower).to_string();
    let segments: Vec<&str> = lower.split('/').collect();
    let mut out: Vec<&'static str> = Vec::new();

    // tests: *_test/*_spec/*.test/*.spec stems, or a test directory segment — for source files only.
    let stem = lower_name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&lower_name);
    let stem_is_test = stem.ends_with("_test")
        || stem.ends_with("_spec")
        || stem.ends_with(".test")
        || stem.ends_with(".spec");
    let in_test_dir = segments
        .iter()
        .any(|s| matches!(*s, "tests" | "test" | "__tests__" | "spec"));
    if (stem_is_test || lower_name.contains(".test.") || lower_name.contains(".spec.") || in_test_dir)
        && is_source_file(path)
    {
        out.push("tests");
    }

    // migrations: .sql under a migrations/ or migrate/ path segment.
    let in_migrations = segments.iter().any(|s| matches!(*s, "migrations" | "migrate"));
    if lower_name.ends_with(".sql") && in_migrations {
        out.push("migrations");
    }

    // config: Dockerfile, Compose, Terraform, or a yaml under .github/workflows/.
    let is_dockerfile = lower_name == "dockerfile" || lower_name.starts_with("dockerfile.");
    let is_tf = lower_name.ends_with(".tf") || lower_name.ends_with(".tfvars");
    let is_compose = matches!(
        lower_name.as_str(),
        "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml"
    );
    let in_workflows = lower.contains(".github/workflows/");
    let is_workflow = in_workflows && (lower_name.ends_with(".yml") || lower_name.ends_with(".yaml"));
    if is_dockerfile || is_tf || is_compose || is_workflow {
        out.push("config");
    }

    out
}

/// True if any specialist (language or overlay) applies to this path.
#[allow(dead_code)]
pub fn is_reviewable(path: &str) -> bool {
    language_for_path(path).is_some() || !overlay_languages_for_path(path).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_for_path() {
        assert_eq!(language_for_path("src/main.rs"), Some("rust"));
        assert_eq!(language_for_path("lib/utils.ts"), Some("typescript"));
        assert_eq!(language_for_path("app.py"), Some("python"));
        assert_eq!(language_for_path("README.md"), None);
    }

    #[test]
    fn test_is_source_file() {
        assert!(is_source_file("src/main.rs"));
        assert!(is_source_file("index.tsx"));
        assert!(!is_source_file("README.md"));
        assert!(!is_source_file("Cargo.toml"));
    }

    #[test]
    fn test_overlay_tests() {
        assert!(overlay_languages_for_path("internal/foo_test.go").contains(&"tests"));
        assert!(overlay_languages_for_path("src/x.test.ts").contains(&"tests"));
        assert!(overlay_languages_for_path("pkg/tests/helpers.py").contains(&"tests"));
        // ordinary source file: no overlay
        assert!(overlay_languages_for_path("src/main.rs").is_empty());
        // non-source file in a test dir: not a test overlay
        assert!(!overlay_languages_for_path("tests/fixtures/data.json").contains(&"tests"));
    }

    #[test]
    fn test_overlay_migrations_and_config() {
        assert!(overlay_languages_for_path("db/migrate/0001_init.sql").contains(&"migrations"));
        assert!(overlay_languages_for_path("migrations/0002_users.sql").contains(&"migrations"));
        assert!(overlay_languages_for_path("Dockerfile").contains(&"config"));
        assert!(overlay_languages_for_path("infra/main.tf").contains(&"config"));
        assert!(overlay_languages_for_path(".github/workflows/ci.yml").contains(&"config"));
        assert!(overlay_languages_for_path("docker-compose.yml").contains(&"config"));
    }

    #[test]
    fn test_overlays_coexist_with_language() {
        assert_eq!(language_for_path("db/migrate/0001_init.sql"), Some("sql"));
        assert!(overlay_languages_for_path("db/migrate/0001_init.sql").contains(&"migrations"));
        assert_eq!(language_for_path("internal/foo_test.go"), Some("go"));
        assert!(overlay_languages_for_path("internal/foo_test.go").contains(&"tests"));
    }

    #[test]
    fn test_is_reviewable() {
        assert!(is_reviewable("Dockerfile"));
        assert!(is_reviewable(".github/workflows/ci.yml"));
        assert!(is_reviewable("src/main.rs"));
        assert!(!is_reviewable("README.md"));
    }
}
