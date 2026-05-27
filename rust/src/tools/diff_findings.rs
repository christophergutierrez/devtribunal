//! `diff_findings` — deterministic comparison of two finding passes (by content
//! id) plus a PASS/FAIL verdict. The convergence engine's accounting: convergence
//! is computed, not narrated. Honors architect overrides (dismiss/downgrade/escalate).
#![allow(dead_code)]

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::json;

use super::ToolResult;
use crate::findings::{from_json, parse_findings_from_markdown, Finding, FindingSet, Severity};

/// Accept either a findings JSON object (`{"findings":[...]}`) or raw agent
/// markdown containing a `## Structured Findings` json block. Lets the skill pass
/// specialist output through with or without pre-extracting the JSON.
fn parse_either(s: &str) -> anyhow::Result<FindingSet> {
    if s.trim_start().starts_with('{') {
        from_json(s)
    } else {
        parse_findings_from_markdown(s)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Override {
    pub finding_id: String,
    pub action: String, // escalate | downgrade | dismiss
    #[serde(default)]
    pub new_severity: Option<String>,
}

fn parse_severity(s: &str) -> Option<Severity> {
    match s.trim().to_lowercase().as_str() {
        "critical" => Some(Severity::Critical),
        "high" => Some(Severity::High),
        "medium" => Some(Severity::Medium),
        "low" => Some(Severity::Low),
        _ => None,
    }
}

struct DiffResult {
    fixed: Vec<String>,
    persisting: Vec<String>,
    new: Vec<String>,
    regressed: Vec<String>,
    pass: bool,
    reasons: Vec<String>,
}

/// Apply overrides to the current findings: dismiss drops, downgrade/escalate remaps severity.
fn apply_overrides(current: &[Finding], overrides: &[Override]) -> Vec<Finding> {
    let mut out = Vec::new();
    for f in current {
        match overrides.iter().find(|o| o.finding_id == f.id) {
            Some(o) if o.action == "dismiss" => continue,
            Some(o) if o.action == "downgrade" || o.action == "escalate" => {
                let mut nf = f.clone();
                if let Some(sev) = o.new_severity.as_deref().and_then(parse_severity) {
                    nf.severity = sev;
                }
                out.push(nf);
            }
            _ => out.push(f.clone()),
        }
    }
    out
}

fn compute(
    previous: &[Finding],
    current: &[Finding],
    previously_fixed: &[String],
    overrides: &[Override],
    block_severities: &[Severity],
    max_new: u32,
) -> DiffResult {
    let cur_adj = apply_overrides(current, overrides);

    let prev_ids: BTreeSet<&str> = previous.iter().map(|f| f.id.as_str()).collect();
    let cur_ids: BTreeSet<&str> = cur_adj.iter().map(|f| f.id.as_str()).collect();
    let fixed_history: BTreeSet<&str> = previously_fixed.iter().map(|s| s.as_str()).collect();

    let dedup_sort = |mut v: Vec<String>| {
        v.sort();
        v.dedup();
        v
    };

    let fixed = dedup_sort(
        previous
            .iter()
            .filter(|f| !cur_ids.contains(f.id.as_str()))
            .map(|f| f.id.clone())
            .collect(),
    );
    let persisting = dedup_sort(
        cur_adj
            .iter()
            .filter(|f| prev_ids.contains(f.id.as_str()))
            .map(|f| f.id.clone())
            .collect(),
    );
    let new = dedup_sort(
        cur_adj
            .iter()
            .filter(|f| !prev_ids.contains(f.id.as_str()))
            .map(|f| f.id.clone())
            .collect(),
    );
    let regressed = dedup_sort(
        new.iter()
            .filter(|id| fixed_history.contains(id.as_str()))
            .cloned()
            .collect(),
    );

    let mut reasons = Vec::new();
    let open_blocking = cur_adj
        .iter()
        .filter(|f| block_severities.contains(&f.severity))
        .count();
    if open_blocking > 0 {
        reasons.push(format!("{open_blocking} open finding(s) at blocking severity"));
    }
    if !regressed.is_empty() {
        reasons.push(format!("{} regression(s)", regressed.len()));
    }
    if new.len() as u32 > max_new {
        reasons.push(format!("{} new finding(s) exceed max_new={}", new.len(), max_new));
    }
    let pass = reasons.is_empty();

    DiffResult { fixed, persisting, new, regressed, pass, reasons }
}

#[allow(clippy::too_many_arguments)]
pub fn execute_diff_findings(
    previous: &str,
    current: &str,
    previously_fixed: &[String],
    overrides: &[Override],
    block_severities: Option<&[String]>,
    max_new: Option<u32>,
) -> ToolResult {
    let prev = match parse_either(previous) {
        Ok(s) => s,
        Err(e) => return ToolResult { content: format!("invalid `previous` findings: {e}"), is_error: true },
    };
    let cur = match parse_either(current) {
        Ok(s) => s,
        Err(e) => return ToolResult { content: format!("invalid `current` findings: {e}"), is_error: true },
    };

    let block: Vec<Severity> = match block_severities {
        Some(list) => list.iter().filter_map(|s| parse_severity(s)).collect(),
        None => vec![Severity::Critical, Severity::High],
    };
    let max_new = max_new.unwrap_or(0);

    let d = compute(&prev.findings, &cur.findings, previously_fixed, overrides, &block, max_new);

    let verdict = if d.pass { "pass" } else { "fail" };
    let body = json!({
        "verdict": verdict,
        "fixed": d.fixed,
        "persisting": d.persisting,
        "new": d.new,
        "regressed": d.regressed,
        "reasons": d.reasons,
    });
    let content = format!(
        "## Findings Diff\n\n- **Verdict:** {}\n- **Fixed:** {} · **Persisting:** {} · **New:** {} · **Regressed:** {}\n{}\n\n```json\n{}\n```",
        verdict.to_uppercase(),
        d.fixed.len(),
        d.persisting.len(),
        d.new.len(),
        d.regressed.len(),
        if d.reasons.is_empty() { String::new() } else { format!("- **Reasons:** {}", d.reasons.join("; ")) },
        serde_json::to_string_pretty(&body).unwrap_or_default(),
    );
    ToolResult { content, is_error: false }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fset(json: &str) -> Vec<Finding> {
        from_json(json).unwrap().findings
    }

    #[test]
    fn parse_either_accepts_markdown_and_json() {
        let json = r#"{"findings":[{"severity":"low","confidence":"possible","category":"c","file":"a.rs","title":"T"}]}"#;
        let md = format!("prose above\n\n## Structured Findings\n\n```json\n{json}\n```\n");
        let from_md = parse_either(&md).unwrap();
        let from_js = parse_either(json).unwrap();
        assert_eq!(from_md.findings.len(), 1);
        assert_eq!(from_md.findings[0].id, from_js.findings[0].id);
    }

    // Helper: a findings JSON object from (file, title, severity) tuples.
    fn mk(items: &[(&str, &str, &str)]) -> String {
        let objs: Vec<String> = items
            .iter()
            .map(|(file, title, sev)| {
                format!(
                    r#"{{"severity":"{sev}","confidence":"likely","category":"c","file":"{file}","title":"{title}"}}"#
                )
            })
            .collect();
        format!(r#"{{"findings":[{}]}}"#, objs.join(","))
    }

    #[test]
    fn classifies_fixed_new_persisting() {
        let prev = fset(&mk(&[("a.rs", "A", "low"), ("b.rs", "B", "low")]));
        let cur = fset(&mk(&[("b.rs", "B", "low"), ("c.rs", "C", "low")]));
        let d = compute(&prev, &cur, &[], &[], &[Severity::Critical, Severity::High], 99);
        assert_eq!(d.fixed.len(), 1); // A gone
        assert_eq!(d.persisting.len(), 1); // B
        assert_eq!(d.new.len(), 1); // C
        assert!(d.regressed.is_empty());
    }

    #[test]
    fn regression_via_previously_fixed() {
        let cur = fset(&mk(&[("a.rs", "A", "low")]));
        let prev: Vec<Finding> = vec![];
        let new_id = cur[0].id.clone();
        let d = compute(&prev, &cur, &[new_id], &[], &[Severity::Critical], 99);
        assert_eq!(d.regressed.len(), 1);
    }

    #[test]
    fn verdict_blocks_on_severity() {
        let prev: Vec<Finding> = vec![];
        let cur = fset(&mk(&[("a.rs", "A", "critical")]));
        let d = compute(&prev, &cur, &[], &[], &[Severity::Critical, Severity::High], 99);
        assert!(!d.pass); // open critical blocks
    }

    #[test]
    fn verdict_passes_when_clean() {
        let prev: Vec<Finding> = vec![];
        let cur = fset(&mk(&[("a.rs", "A", "low")]));
        let d = compute(&prev, &cur, &[], &[], &[Severity::Critical, Severity::High], 1);
        assert!(d.pass); // low severity, new=1 <= max_new=1, no regressions
    }

    #[test]
    fn max_new_threshold() {
        let prev: Vec<Finding> = vec![];
        let cur = fset(&mk(&[("a.rs", "A", "low"), ("b.rs", "B", "low")]));
        let d = compute(&prev, &cur, &[], &[], &[Severity::Critical], 1);
        assert!(!d.pass); // 2 new > max_new 1
    }

    #[test]
    fn override_dismiss_clears_block() {
        let prev: Vec<Finding> = vec![];
        let cur = fset(&mk(&[("a.rs", "A", "critical")]));
        let ov = vec![Override { finding_id: cur[0].id.clone(), action: "dismiss".into(), new_severity: None }];
        let d = compute(&prev, &cur, &[], &ov, &[Severity::Critical, Severity::High], 99);
        assert!(d.pass); // dismissed critical no longer blocks
        assert!(d.new.is_empty());
    }

    #[test]
    fn override_downgrade_changes_verdict() {
        let prev: Vec<Finding> = vec![];
        let cur = fset(&mk(&[("a.rs", "A", "critical")]));
        let ov = vec![Override { finding_id: cur[0].id.clone(), action: "downgrade".into(), new_severity: Some("low".into()) }];
        let d = compute(&prev, &cur, &[], &ov, &[Severity::Critical, Severity::High], 99);
        assert!(d.pass); // downgraded to low, no longer blocking
    }

    #[test]
    fn deterministic_sorted_output() {
        let prev: Vec<Finding> = vec![];
        let cur = fset(&mk(&[("z.rs", "Z", "low"), ("a.rs", "A", "low")]));
        let d1 = compute(&prev, &cur, &[], &[], &[Severity::Critical], 99);
        let d2 = compute(&prev, &cur, &[], &[], &[Severity::Critical], 99);
        assert_eq!(d1.new, d2.new);
        let mut sorted = d1.new.clone();
        sorted.sort();
        assert_eq!(d1.new, sorted);
    }
}
