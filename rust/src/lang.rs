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
}
