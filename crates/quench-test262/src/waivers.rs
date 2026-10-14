//! Node-arbitrated waivers: the checked-in authority for pinned cases whose
//! raw Quench failure the local Node oracle reproduces.
//!
//! A waiver never turns a failure into a pass. Reports keep the raw outcome;
//! runners accept a waived case only while it fails with exactly its recorded
//! failure, and reject a waiver whose case now passes or fails differently.
use std::collections::BTreeMap;

use crate::ratchet::RATCHET_ENGINE;

pub const WAIVER_SCHEMA_VERSION: u64 = 1;
const MANIFEST: &str = include_str!("../node-waivers.json");

/// How a runner must treat one case's raw outcome.
#[derive(Debug, PartialEq, Eq)]
pub enum Adjudication<'a> {
    Pass,
    /// A failure matching its waiver; the value names the waiver group.
    Waived(&'a str),
    /// A failure the runner must report, with the reason to show for it.
    Failed(String),
}

impl<'a> Adjudication<'a> {
    /// The waiver group a report records for this outcome.
    pub fn waiver(&self) -> Option<&'a str> {
        match self {
            Self::Waived(group) => Some(group),
            _ => None,
        }
    }
}

struct Waiver {
    group: String,
    failure: String,
}

pub struct Waivers {
    cases: BTreeMap<String, Waiver>,
}

impl Waivers {
    /// The manifest compiled into this runner.
    pub fn pinned() -> Result<Self, String> {
        let manifest = serde_json::from_str(MANIFEST)
            .map_err(|error| format!("parse Test262 Node waivers: {error}"))?;
        Self::from_json(&manifest)
    }

    pub fn from_json(manifest: &serde_json::Value) -> Result<Self, String> {
        if manifest["schema"] != WAIVER_SCHEMA_VERSION || manifest["engine"] != RATCHET_ENGINE {
            return Err("Test262 Node waivers have an unsupported schema or engine".into());
        }
        let groups = manifest["groups"]
            .as_array()
            .ok_or("Test262 Node waivers have no groups")?;
        let mut cases = BTreeMap::new();
        for group in groups {
            let id = group["id"]
                .as_str()
                .filter(|id| !id.is_empty())
                .ok_or("Test262 Node waiver group has no id")?;
            if group["reason"].as_str().is_none_or(str::is_empty) {
                return Err(format!("Test262 Node waiver group {id} has no reason"));
            }
            let entries = group["cases"]
                .as_array()
                .filter(|entries| !entries.is_empty())
                .ok_or_else(|| format!("Test262 Node waiver group {id} has no cases"))?;
            for entry in entries {
                let (Some(path), Some(failure)) = (
                    entry["path"].as_str().filter(|path| !path.is_empty()),
                    entry["failure"]
                        .as_str()
                        .filter(|failure| !failure.is_empty()),
                ) else {
                    return Err(format!(
                        "Test262 Node waiver in {id} lacks a path or failure"
                    ));
                };
                let waiver = Waiver {
                    group: id.to_owned(),
                    failure: failure.to_owned(),
                };
                if cases.insert(path.to_owned(), waiver).is_some() {
                    return Err(format!("Test262 Node waiver duplicates {path}"));
                }
            }
        }
        Ok(Self { cases })
    }

    /// The waiver group of `path`, if one is recorded.
    pub fn group(&self, path: &str) -> Option<&str> {
        self.cases.get(path).map(|waiver| waiver.group.as_str())
    }

    pub fn adjudicate(&self, path: &str, outcome: &Result<(), String>) -> Adjudication<'_> {
        match (outcome, self.cases.get(path)) {
            (Ok(()), None) => Adjudication::Pass,
            (Ok(()), Some(waiver)) => Adjudication::Failed(format!(
                "waived case now passes; remove it from Node waiver group {}",
                waiver.group
            )),
            (Err(reason), Some(waiver)) if *reason == waiver.failure => {
                Adjudication::Waived(&waiver.group)
            }
            (Err(reason), _) => Adjudication::Failed(reason.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waivers() -> Waivers {
        Waivers::from_json(&serde_json::json!({
            "schema":1,"engine":"quench","groups":[{"id":"order","reason":"Node order",
            "cases":[{"path":"a.js","failure":"FAIL: expected"}]}]
        }))
        .unwrap()
    }

    #[test]
    fn waived_cases_are_accepted_only_with_their_recorded_failure() {
        let waivers = waivers();
        assert_eq!(waivers.adjudicate("b.js", &Ok(())), Adjudication::Pass);
        assert_eq!(
            waivers.adjudicate("a.js", &Err("FAIL: expected".into())),
            Adjudication::Waived("order")
        );
        assert_eq!(
            waivers.adjudicate("a.js", &Err("FAIL: other".into())),
            Adjudication::Failed("FAIL: other".into())
        );
        assert!(matches!(
            waivers.adjudicate("a.js", &Ok(())),
            Adjudication::Failed(reason) if reason.contains("now passes")
        ));
        assert_eq!(
            waivers.adjudicate("b.js", &Err("FAIL: x".into())),
            Adjudication::Failed("FAIL: x".into())
        );
    }

    #[test]
    fn malformed_manifests_are_rejected() {
        for manifest in [
            serde_json::json!({"schema":2,"engine":"quench","groups":[]}),
            serde_json::json!({"schema":1,"engine":"quench"}),
            serde_json::json!({"schema":1,"engine":"quench","groups":[{"id":"g","reason":"r","cases":[]}]}),
            serde_json::json!({"schema":1,"engine":"quench","groups":[{"id":"g","cases":[{"path":"a","failure":"f"}]}]}),
            serde_json::json!({"schema":1,"engine":"quench","groups":[{"id":"g","reason":"r","cases":[{"path":"a"}]}]}),
            serde_json::json!({"schema":1,"engine":"quench","groups":[
                {"id":"g","reason":"r","cases":[{"path":"a","failure":"f"}]},
                {"id":"h","reason":"r","cases":[{"path":"a","failure":"f"}]}]}),
        ] {
            assert!(Waivers::from_json(&manifest).is_err());
        }
    }

    #[test]
    fn pinned_manifest_is_valid() {
        assert!(Waivers::pinned().is_ok());
    }
}
