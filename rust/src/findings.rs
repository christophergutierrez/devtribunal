//! Structured findings contract (v0.8 convergence engine).
//!
//! Atomic review findings with content-based, line-independent IDs so the same
//! issue matches across review passes even after edits shift line numbers.
//!
//! `#![allow(dead_code)]` is temporary: Phase 14 (`diff_findings`) and Phase 19
//! (`dt:converge`) consume these items. Remove the allow once they are wired in.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Confirmed,
    Likely,
    Possible,
}

/// A single atomic review finding. `id` is never trusted from input — it is
/// always (re)assigned by `from_json` from the finding's content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    #[serde(default)]
    pub id: String,
    pub severity: Severity,
    pub confidence: Confidence,
    pub category: String,
    pub file: String,
    #[serde(default)]
    pub line: Option<u32>,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub suggested_fix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindingSet {
    pub findings: Vec<Finding>,
}

/// Normalize a string for identity hashing: trim, lowercase, collapse internal
/// whitespace to single spaces, strip a single trailing period.
fn normalize(s: &str) -> String {
    let lowered = s.trim().to_lowercase();
    let collapsed = lowered.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.trim_end_matches('.').to_string()
}

/// Content-based, line-independent finding id.
///
/// FNV-1a-64 over `normalize(file) \x1f lower(category) \x1f normalize(title)`,
/// rendered as `F-<12 hex>`. Hand-rolled (no crate) and stable across releases —
/// `std`'s `DefaultHasher` is explicitly NOT stable across versions, which would
/// break cross-pass matching. Line is deliberately excluded from identity.
fn fnv1a_id(file: &str, category: &str, title: &str) -> String {
    let key = format!(
        "{}\u{1f}{}\u{1f}{}",
        normalize(file),
        category.trim().to_lowercase(),
        normalize(title),
    );
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("F-{:012x}", hash & 0xffff_ffff_ffff)
}

/// Parse and validate a findings JSON object, assigning content-based ids.
pub fn from_json(json: &str) -> anyhow::Result<FindingSet> {
    let mut set: FindingSet =
        serde_json::from_str(json).map_err(|e| anyhow::anyhow!("invalid findings JSON: {e}"))?;
    for (i, f) in set.findings.iter_mut().enumerate() {
        if f.file.trim().is_empty() {
            anyhow::bail!("finding[{i}]: 'file' must be non-empty");
        }
        if f.title.trim().is_empty() {
            anyhow::bail!("finding[{i}]: 'title' must be non-empty");
        }
        if f.category.trim().is_empty() {
            anyhow::bail!("finding[{i}]: 'category' must be non-empty");
        }
        f.id = fnv1a_id(&f.file, &f.category, &f.title);
    }
    Ok(set)
}

/// Extract the structured findings block from an agent's markdown output.
///
/// Scans fenced ```json blocks (preferring any that follow a `## Structured
/// Findings` heading) and returns the first that decodes to a `{ "findings": [...] }`.
pub fn parse_findings_from_markdown(md: &str) -> anyhow::Result<FindingSet> {
    let re = regex::RegexBuilder::new(r"```json\s*\n(.*?)\n```")
        .dot_matches_new_line(true)
        .build()
        .expect("valid findings regex");

    let mut last_err: Option<anyhow::Error> = None;
    for cap in re.captures_iter(md) {
        let block = &cap[1];
        if !block.contains("\"findings\"") {
            continue;
        }
        match from_json(block) {
            Ok(set) => return Ok(set),
            Err(e) => last_err = Some(e),
        }
    }

    match last_err {
        Some(e) => Err(anyhow::anyhow!(
            "found a findings json block but it failed to parse: {e}"
        )),
        None => anyhow::bail!("no `findings` json block found in agent output"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_MD: &str = r#"
Some prose review above.

## Structured Findings

```json
{
  "findings": [
    {
      "severity": "critical",
      "confidence": "confirmed",
      "category": "memory_safety",
      "file": "src/buffer.c",
      "line": 88,
      "title": "Unbounded strcpy into fixed buffer",
      "description": "User input copied without a length check.",
      "suggested_fix": "Use strncpy with sizeof(buf) - 1."
    }
  ]
}
```
"#;

    #[test]
    fn parses_valid_block_and_fields() {
        let set = parse_findings_from_markdown(VALID_MD).unwrap();
        assert_eq!(set.findings.len(), 1);
        let f = &set.findings[0];
        assert_eq!(f.severity, Severity::Critical);
        assert_eq!(f.confidence, Confidence::Confirmed);
        assert_eq!(f.category, "memory_safety");
        assert_eq!(f.file, "src/buffer.c");
        assert_eq!(f.line, Some(88));
        assert!(f.id.starts_with("F-"));
        assert_eq!(f.id.len(), 14); // "F-" + 12 hex
    }

    #[test]
    fn id_is_content_based_and_line_independent() {
        // Same file/category/title, different line + description => same id.
        let a = from_json(
            r#"{"findings":[{"severity":"high","confidence":"likely","category":"perf","file":"a.rs","line":10,"title":"N+1 query","description":"x"}]}"#,
        )
        .unwrap();
        let b = from_json(
            r#"{"findings":[{"severity":"high","confidence":"likely","category":"perf","file":"a.rs","line":42,"title":"N+1 Query.","description":"different"}]}"#,
        )
        .unwrap();
        assert_eq!(a.findings[0].id, b.findings[0].id, "line/description/case must not affect id");

        // Different title => different id.
        let c = from_json(
            r#"{"findings":[{"severity":"high","confidence":"likely","category":"perf","file":"a.rs","title":"Unbounded loop","description":"x"}]}"#,
        )
        .unwrap();
        assert_ne!(a.findings[0].id, c.findings[0].id);
    }

    #[test]
    fn line_may_be_absent() {
        let set = from_json(
            r#"{"findings":[{"severity":"low","confidence":"possible","category":"style","file":"a.rs","title":"t","description":"d"}]}"#,
        )
        .unwrap();
        assert_eq!(set.findings[0].line, None);
    }

    #[test]
    fn invalid_severity_errors() {
        let err = from_json(
            r#"{"findings":[{"severity":"blocker","confidence":"likely","category":"x","file":"a.rs","title":"t"}]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("invalid findings JSON"));
    }

    #[test]
    fn missing_required_field_errors() {
        // Missing "file".
        let err = from_json(
            r#"{"findings":[{"severity":"low","confidence":"possible","category":"x","title":"t"}]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("invalid findings JSON"));
    }

    #[test]
    fn empty_required_field_errors() {
        let err = from_json(
            r#"{"findings":[{"severity":"low","confidence":"possible","category":"x","file":"   ","title":"t"}]}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("'file' must be non-empty"));
    }

    #[test]
    fn no_json_block_errors_gracefully() {
        let err = parse_findings_from_markdown("just prose, no block").unwrap_err();
        assert!(err.to_string().contains("no `findings` json block"));
    }

    #[test]
    fn empty_findings_set_is_valid() {
        let set = from_json(r#"{"findings":[]}"#).unwrap();
        assert!(set.findings.is_empty());
    }
}
