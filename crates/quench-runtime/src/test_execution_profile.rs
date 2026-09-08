//! Invocation-local execution profiles for source-level stencil contracts.
//!
//! This module exists only in unit-test builds. Production execution retains
//! no counters, environment switches, or benchmark-facing behavior.

use serde::Deserialize;
use std::{cell::RefCell, collections::BTreeMap, path::PathBuf};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
pub(crate) struct RouteCount {
    pub(crate) entries: u64,
    pub(crate) fallbacks: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExecutionProfile {
    pub(crate) residual_ops: BTreeMap<&'static str, u64>,
    pub(crate) slow_ops: BTreeMap<&'static str, u64>,
    pub(crate) stencils: BTreeMap<&'static str, RouteCount>,
    pub(crate) events: BTreeMap<&'static str, u64>,
}

const EXECUTION_CASE_SCHEMA: u32 = 1;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionCase {
    schema: u32,
    warmup: u32,
    result: ExpectedValue,
    plan: ExpectedPlan,
    profile: ExpectedProfile,
    #[serde(default)]
    scaled: Vec<ScaledExpectation>,
    #[serde(skip)]
    source: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ExpectedValue {
    Number { value: f64 },
    String { value: String },
    Boolean { value: bool },
    Undefined,
    Null,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedPlan {
    execution_kind: ExecutionKind,
    operation_route: Vec<String>,
    fallback: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ExecutionKind {
    NativeMachineCode,
    PortableRecipe,
    OrdinaryFallback,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedProfile {
    #[serde(default)]
    residual: BTreeMap<String, u64>,
    #[serde(default)]
    slow: BTreeMap<String, u64>,
    #[serde(default)]
    stencils: BTreeMap<String, RouteCount>,
    #[serde(default)]
    events: BTreeMap<String, u64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScaledExpectation {
    executions: u64,
    #[serde(default)]
    events: BTreeMap<String, u64>,
}

impl ExecutionCase {
    pub(crate) fn load(name: &str) -> Self {
        let root = fixture_root();
        let source = std::fs::read_to_string(root.join(format!("{name}.js")))
            .unwrap_or_else(|error| panic!("cannot read {name}.js: {error}"));
        let json = std::fs::read_to_string(root.join(format!("{name}.json")))
            .unwrap_or_else(|error| panic!("cannot read {name}.json: {error}"));
        let mut case: Self = serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("invalid {name}.json: {error}"));
        assert_eq!(
            case.schema, EXECUTION_CASE_SCHEMA,
            "unsupported case schema"
        );
        case.source = source;
        case
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn assert(&self, result: &crate::value::Value, profile: &ExecutionProfile) {
        self.result.assert(result, &self.source);
        self.profile.assert(profile);
        for expected in &self.scaled {
            expected.assert(profile);
        }
    }

    pub(crate) fn assert_standalone(&self) {
        let program = crate::reduce::reduce_source(&self.source)
            .unwrap_or_else(|errors| panic!("case does not lower: {}", errors.join("; ")));
        let context = crate::vm::current_context_or_default();
        let result = crate::vm::execute_code_with_context(program.code(), &context)
            .unwrap_or_else(|error| panic!("standalone case failed: {error:?}"));
        self.result.assert(&result, &self.source);
    }

    pub(crate) const fn warmup(&self) -> u32 {
        self.warmup
    }

    pub(crate) fn assert_plan(&self, kind: ExecutionKind, route: &[&str]) {
        assert_eq!(kind, self.plan.execution_kind);
        assert_eq!(route, self.plan.operation_route);
        assert_eq!(self.plan.fallback, kind == ExecutionKind::OrdinaryFallback);
    }
}

impl ExpectedValue {
    fn assert(&self, actual: &crate::value::Value, source: &str) {
        let matches = match (self, actual) {
            (Self::Number { value }, crate::value::Value::Number(actual)) => value == actual,
            (Self::String { value }, crate::value::Value::String(actual)) => value == actual,
            (Self::Boolean { value }, crate::value::Value::Boolean(actual)) => value == actual,
            (Self::Undefined, crate::value::Value::Undefined) => true,
            (Self::Null, crate::value::Value::Null) => true,
            _ => false,
        };
        assert!(matches, "wrong JS result for {source}: {actual:?}");
    }
}

impl ExpectedProfile {
    fn assert(&self, actual: &ExecutionProfile) {
        assert_eq!(string_counts(&actual.residual_ops), self.residual);
        assert_eq!(string_counts(&actual.slow_ops), self.slow);
        assert_eq!(string_routes(&actual.stencils), self.stencils);
        assert_eq!(string_counts(&actual.events), self.events);
    }
}

impl ScaledExpectation {
    fn assert(&self, profile: &ExecutionProfile) {
        let scaled = profile.scaled(self.executions);
        for (name, expected) in &self.events {
            assert_eq!(
                scaled.events.get(name.as_str()),
                Some(expected),
                "scaled {name}"
            );
        }
    }
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/execution_profiles")
}

fn fixture_names() -> Vec<String> {
    let mut names = std::fs::read_dir(fixture_root())
        .expect("execution-profile fixture directory")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            (entry.path().extension()?.to_str()? == "json")
                .then(|| entry.path().file_stem()?.to_str().map(str::to_owned))?
        })
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn string_counts(input: &BTreeMap<&'static str, u64>) -> BTreeMap<String, u64> {
    input
        .iter()
        .map(|(&name, &count)| (name.into(), count))
        .collect()
}

fn string_routes(input: &BTreeMap<&'static str, RouteCount>) -> BTreeMap<String, RouteCount> {
    input
        .iter()
        .map(|(&name, &count)| (name.into(), count))
        .collect()
}

impl ExecutionProfile {
    pub(crate) fn scaled(&self, executions: u64) -> Self {
        Self {
            residual_ops: scaled_counts(&self.residual_ops, executions),
            slow_ops: scaled_counts(&self.slow_ops, executions),
            stencils: self
                .stencils
                .iter()
                .map(|(&name, count)| {
                    (
                        name,
                        RouteCount {
                            entries: count.entries.saturating_mul(executions),
                            fallbacks: count.fallbacks.saturating_mul(executions),
                        },
                    )
                })
                .collect(),
            events: scaled_counts(&self.events, executions),
        }
    }
}

thread_local! {
    static ACTIVE: RefCell<Option<ExecutionProfile>> = const { RefCell::new(None) };
}

struct CaptureGuard;

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        ACTIVE.with(|active| {
            active.borrow_mut().take();
        });
    }
}

pub(crate) fn capture<R>(execute: impl FnOnce() -> R) -> (R, ExecutionProfile) {
    ACTIVE.with(|active| {
        assert!(
            active.borrow().is_none(),
            "execution profile capture cannot nest"
        );
        *active.borrow_mut() = Some(ExecutionProfile::default());
    });
    let guard = CaptureGuard;
    let result = execute();
    let profile = ACTIVE.with(|active| active.borrow_mut().take().unwrap());
    drop(guard);
    (result, profile)
}

pub(crate) fn residual(name: &'static str) {
    update(|profile| increment(&mut profile.residual_ops, name));
}

pub(crate) fn slow(name: &'static str) {
    update(|profile| increment(&mut profile.slow_ops, name));
}

pub(crate) fn stencil(name: &'static str, entered: bool) {
    update(|profile| {
        let count = profile.stencils.entry(name).or_default();
        if entered {
            count.entries += 1;
        } else {
            count.fallbacks += 1;
        }
    });
}

pub(crate) fn event(name: &'static str) {
    update(|profile| increment(&mut profile.events, name));
}

fn update(apply: impl FnOnce(&mut ExecutionProfile)) {
    ACTIVE.with(|active| {
        if let Some(profile) = active.borrow_mut().as_mut() {
            apply(profile);
        }
    });
}

fn increment(counts: &mut BTreeMap<&'static str, u64>, name: &'static str) {
    *counts.entry(name).or_default() += 1;
}

fn scaled_counts(
    counts: &BTreeMap<&'static str, u64>,
    executions: u64,
) -> BTreeMap<&'static str, u64> {
    counts
        .iter()
        .map(|(&name, count)| (name, count.saturating_mul(executions)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_is_invocation_local_and_deterministic() {
        let (_, first) = capture(|| {
            residual("Add");
            stencil("add", true);
        });
        let (_, second) = capture(|| residual("Return"));
        assert_eq!(first.residual_ops.get("Add"), Some(&1));
        assert_eq!(first.stencils.get("add").unwrap().entries, 1);
        assert_eq!(second.residual_ops.get("Return"), Some(&1));
        assert!(!second.stencils.contains_key("add"));
    }

    #[test]
    fn every_json_contract_has_a_complete_standalone_js_case() {
        let names = fixture_names();
        assert!(!names.is_empty(), "execution-profile cases must exist");
        for name in names {
            let case = ExecutionCase::load(&name);
            assert!(!case.source().trim().is_empty(), "empty JS case: {name}");
            case.assert_standalone();
        }
    }
}
