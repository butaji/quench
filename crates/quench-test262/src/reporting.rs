//! Durable outcome views and comparisons; case outcomes are the report authority.
use std::collections::{BTreeMap, BTreeSet};
use serde_json::{Value, json};

pub fn normalize_failure(reason: &str) -> String {
    let message = ["text: ", "message: "].into_iter().find_map(|marker| {
        let (_, rest) = reason.split_once(marker)?;
        serde_json::Deserializer::from_str(rest).into_iter::<String>().next()?.ok()
    }).unwrap_or_else(|| reason.lines().next().unwrap_or("unknown failure").to_owned());
    let mut normalized = String::new();
    let mut chars = message.chars().peekable();
    while let Some(character) = chars.next() {
        if character.is_ascii_digit() {
            normalized.push('#');
            while chars.peek().is_some_and(char::is_ascii_digit) { chars.next(); }
        } else if let Some((close, replacement)) = match character {
            '\'' => Some(('\'', "'…'")),
            '"' => Some(('"', "\"…\"")),
            '«' => Some(('»', "«…»")),
            _ => None,
        } {
            normalized.push_str(replacement);
            for next in chars.by_ref() { if next == close { break; } }
        } else { normalized.push(character); }
    }
    normalized
}

/// Preserve signal termination even when a worker produces no diagnostic.
pub fn process_failure(status: std::process::ExitStatus, stderr: String) -> String {
    let reason = stderr.trim();
    if status.code().is_none() || reason.is_empty() {
        format!("case process exited with {status}: {reason}")
    } else { reason.to_owned() }
}

pub fn classify_outcome(reason: &str) -> &'static str {
    if reason.starts_with("timed_out") || reason.starts_with("test timed out") {
        "timed_out"
    } else if reason.contains("process exited") || reason.contains("signal:") || reason.contains("panicked at") {
        "crashed"
    } else { "failed" }
}

pub fn case_outcome(path: String, stage: String, result: &Result<(), String>) -> Value {
    match result {
        Ok(()) => json!({"path":path,"stage":stage,"outcome":"pass"}),
        Err(reason) => json!({"path":path,"stage":stage,"outcome":classify_outcome(reason),"reason":reason}),
    }
}

pub fn outcome_report(outcomes: &[Value], provenance: Value) -> Value {
    let passed = outcomes.iter().filter(|case| case["outcome"] == "pass").count();
    let mut stages = BTreeMap::<String, (usize, usize)>::new();
    let mut families = BTreeMap::<String, usize>::new();
    for case in outcomes {
        let counts = stages.entry(case["stage"].as_str().unwrap().to_owned()).or_default();
        counts.0 += 1;
        counts.1 += usize::from(case["outcome"] == "pass");
        if let Some(reason) = case["reason"].as_str() {
            *families.entry(normalize_failure(reason)).or_default() += 1;
        }
    }
    let stages = stages.into_iter().map(|(stage, (total, passed))| {
        (stage, json!({"passed":passed,"total":total,"failed":total-passed}))
    }).collect::<BTreeMap<_, _>>();
    json!({"schema":1,"engine":"next","provenance":provenance,
        "total":outcomes.len(),"passed":passed,"failed":outcomes.len()-passed,
        "stages":stages,"families":families,"outcomes":outcomes})
}

fn pass_paths(report: &Value) -> Result<BTreeSet<String>, String> {
    if report["schema"] != 1 || report["engine"] != "next" || !report["provenance"].is_object() {
        return Err("expected a next-core outcome report with provenance".into());
    }
    let outcomes = report["outcomes"].as_array().filter(|cases| !cases.is_empty())
        .ok_or("outcome report has no cases")?;
    if report["total"].as_u64() != Some(outcomes.len() as u64) {
        return Err("outcome report is incomplete".into());
    }
    let mut seen = BTreeSet::new();
    let mut passes = BTreeSet::new();
    for case in outcomes {
        let path = case["path"].as_str().ok_or("case has no path")?;
        if !seen.insert(path) { return Err(format!("duplicate case: {path}")); }
        match case["outcome"].as_str() {
            Some("pass") => { passes.insert(path.to_owned()); }
            Some("failed" | "crashed" | "timed_out") => {}
            _ => return Err(format!("unknown outcome: {path}")),
        }
    }
    Ok(passes)
}

/// Missing formerly passing cases are regressions too, including deleted tests.
pub fn compare_reports(before: &Value, after: &Value) -> Result<Vec<String>, String> {
    let baseline = pass_paths(before)?;
    let current = pass_paths(after)?;
    Ok(baseline.difference(&current).cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn empty_worker_stderr_preserves_signal_failure() {
        use std::os::unix::process::ExitStatusExt;
        let reason = process_failure(std::process::ExitStatus::from_raw(9), String::new());
        assert!(reason.contains("signal"));
        assert_eq!(classify_outcome(&reason), "crashed");
        let reason = process_failure(std::process::ExitStatus::from_raw(1 << 8), "TypeError: bad receiver".into());
        assert_eq!(reason, "TypeError: bad receiver");
        assert_eq!(classify_outcome(&reason), "failed");
    }
    #[test]
    fn preserves_process_failure_classifications() {
        for (reason, expected) in [("test timed out after 30ms", "timed_out"),
            ("timed_out after 1ms", "timed_out"), ("case process exited with signal: 6", "crashed"),
            ("thread panicked at source.rs", "crashed"), ("TypeError: bad receiver", "failed")] {
            assert_eq!(classify_outcome(reason), expected);
        }
    }
    #[test]
    fn clusters_message_without_debug_heap_identity() {
        for value in [17, 92] {
            let reason = format!("JsError {{ text: \"Expected SameValue(«{value}», «false») at 'fixture-{value}.js'\", thrown: Some(heap({value})) }}");
            assert_eq!(normalize_failure(&reason), "Expected SameValue(«…», «…») at '…'");
        }
    }
    #[test]
    fn report_comparison_detects_failures_timeouts_crashes_and_missing_passes() {
        let before = outcome_report(&[json!({"path":"a","stage":"114","outcome":"pass"}),json!({"path":"b","stage":"114","outcome":"pass"})], json!({"source_revision":"before"}));
        for outcome in ["failed", "timed_out", "crashed"] {
            let after = outcome_report(&[json!({"path":"a","stage":"114","outcome":outcome,"reason":"failure 42"})],json!({"source_revision":"after"}));
            assert_eq!(compare_reports(&before, &after).unwrap(), ["a", "b"]);
        }
        assert!(compare_reports(&before,&before).unwrap().is_empty());
        let mut incomplete = before.clone(); incomplete["total"] = json!(3);
        assert!(compare_reports(&before,&incomplete).is_err());
        let mut duplicate = before.clone(); duplicate["outcomes"][1] = duplicate["outcomes"][0].clone();
        assert!(compare_reports(&before,&duplicate).is_err());
    }
}
