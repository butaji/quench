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
    region_routes: Vec<Vec<&'static str>>,
    lowered_route: Vec<&'static str>,
}

const EXECUTION_CASE_SCHEMA: u32 = 1;
const PROFILE_RUN_PROPERTY: &str = "run";
const PROFILE_VERIFY_PROPERTY: &str = "verify";
const PROFILE_ARGUMENTS_PROPERTY: &str = "arguments";
const PROFILE_CASE_FILTER: &str = "QUENCH_EXECUTION_PROFILE_CASE";
const PROFILE_CHILD_PROCESS: &str = "QUENCH_EXECUTION_PROFILE_CHILD";
const PROFILE_MISMATCH_MARKER: &str = "QUENCH_PROFILE_MISMATCH:";

struct PreparedExecution {
    run: crate::value::Value,
    verify: crate::value::Value,
    arguments: Vec<crate::value::Value>,
}

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
    Nan,
    PositiveInfinity,
    NegativeInfinity,
    NegativeZero,
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
        let result = execute_contract(program.code(), &context)
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

fn prepare_execution(
    value: &crate::value::Value,
) -> Result<Option<PreparedExecution>, crate::execute::VmError> {
    if !matches!(value, crate::value::Value::Object(_)) {
        return Ok(None);
    }
    let run = crate::execute::get_property_result(value, PROFILE_RUN_PROPERTY)?;
    let verify = crate::execute::get_property_result(value, PROFILE_VERIFY_PROPERTY)?;
    if !crate::conversion::is_callable(&run) || !crate::conversion::is_callable(&verify) {
        return Ok(None);
    }
    let arguments = profile_arguments(value)?;
    Ok(Some(PreparedExecution {
        run,
        verify,
        arguments,
    }))
}

fn profile_arguments(
    contract: &crate::value::Value,
) -> Result<Vec<crate::value::Value>, crate::execute::VmError> {
    let value = crate::execute::get_property_result(contract, PROFILE_ARGUMENTS_PROPERTY)?;
    match value {
        crate::value::Value::Undefined => Ok(Vec::new()),
        crate::value::Value::Array(values) => values
            .packed_values()
            .ok_or_else(|| crate::execute::type_error("profile arguments must be a packed array")),
        _ => Err(crate::execute::type_error(
            "profile arguments must be an array",
        )),
    }
}

fn settle(value: crate::value::Value) -> Result<crate::value::Value, crate::execute::VmError> {
    let crate::value::Value::Promise(promise) = value else {
        return Ok(value);
    };
    crate::promise::drain_microtasks_all();
    let state = promise.state.borrow().clone();
    match state {
        crate::value::PromiseState::Fulfilled(value) => Ok(value),
        crate::value::PromiseState::Rejected(value) => Err(crate::execute::VmError::Thrown(value)),
        crate::value::PromiseState::Pending => Err(crate::execute::VmError::Suspended(promise)),
    }
}

fn invoke(
    context: &crate::vm::VmContext,
    function: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let result = crate::vm::with_current_context(context, || {
        crate::execute::call(function, &crate::value::Value::Undefined, arguments)
    })?;
    crate::vm::with_current_context(context, || settle(result))
}

fn execute_contract(
    code: crate::machine::CodeView<'_>,
    context: &crate::vm::VmContext,
) -> Result<crate::value::Value, crate::execute::VmError> {
    let initialized = crate::vm::execute_code_with_context(code, context)?;
    let Some(prepared) = prepare_execution(&initialized)? else {
        return Ok(initialized);
    };
    let result = invoke(context, &prepared.run, &prepared.arguments)?;
    invoke(context, &prepared.verify, &[result])
}

impl ExpectedValue {
    fn matches(&self, actual: &crate::value::Value) -> bool {
        match (self, actual) {
            (Self::Number { value }, crate::value::Value::Number(actual)) => value == actual,
            (Self::Nan, crate::value::Value::Number(actual)) => actual.is_nan(),
            (Self::PositiveInfinity, crate::value::Value::Number(actual)) => {
                *actual == f64::INFINITY
            }
            (Self::NegativeInfinity, crate::value::Value::Number(actual)) => {
                *actual == f64::NEG_INFINITY
            }
            (Self::NegativeZero, crate::value::Value::Number(actual)) => {
                *actual == 0.0 && actual.is_sign_negative()
            }
            (Self::String { value }, crate::value::Value::String(actual)) => value == actual,
            (Self::Boolean { value }, crate::value::Value::Boolean(actual)) => value == actual,
            (Self::Undefined, crate::value::Value::Undefined) => true,
            (Self::Null, crate::value::Value::Null) => true,
            _ => false,
        }
    }

    fn assert(&self, actual: &crate::value::Value, source: &str) {
        assert!(
            self.matches(actual),
            "wrong JS result for {source}: {actual:?}"
        );
    }
}

impl ExpectedProfile {
    fn assert(&self, actual: &ExecutionProfile) {
        assert_eq!(string_counts(&actual.residual_ops), self.residual);
        assert_eq!(string_counts(&actual.slow_ops), self.slow);
        assert_eq!(string_routes(&actual.stencils), self.stencils);
        assert_eq!(string_counts(&actual.events), self.events);
    }

    fn differences(&self, actual: &ExecutionProfile) -> Vec<String> {
        let comparisons = [
            (
                "residual",
                string_counts(&actual.residual_ops),
                self.residual.clone(),
            ),
            ("slow", string_counts(&actual.slow_ops), self.slow.clone()),
            ("events", string_counts(&actual.events), self.events.clone()),
        ];
        let mut differences = comparisons
            .into_iter()
            .filter(|(_, actual, expected)| actual != expected)
            .map(|(name, actual, expected)| {
                format!("{name}: expected {expected:?}, actual {actual:?}")
            })
            .collect::<Vec<_>>();
        let actual = string_routes(&actual.stencils);
        if actual != self.stencils {
            differences.push(format!(
                "stencils: expected {:?}, actual {actual:?}",
                self.stencils
            ));
        }
        differences
    }
}

impl ExpectedPlan {
    fn differences(&self, actual: &ExecutionProfile) -> Vec<String> {
        let kind = actual.execution_kind();
        let route = actual.region_routes.first().cloned().unwrap_or_default();
        let route = route.into_iter().map(str::to_owned).collect::<Vec<_>>();
        let fallback = kind == ExecutionKind::OrdinaryFallback;
        let mut differences = Vec::new();
        if kind != self.execution_kind {
            differences.push(format!(
                "plan kind: expected {:?}, actual {kind:?}",
                self.execution_kind
            ));
        }
        if route != self.operation_route {
            differences.push(format!(
                "plan route: expected {:?}, actual {route:?}, lowered {:?}",
                self.operation_route, actual.lowered_route
            ));
        }
        if fallback != self.fallback {
            differences.push(format!(
                "plan fallback: expected {}, actual {fallback}",
                self.fallback
            ));
        }
        differences
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
    match std::env::var(PROFILE_CASE_FILTER) {
        Ok(filter) => names
            .into_iter()
            .filter(|name| name.contains(&filter))
            .collect(),
        Err(_) => names,
    }
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
    fn execution_kind(&self) -> ExecutionKind {
        if self.stencils.values().any(|count| count.entries != 0) {
            return ExecutionKind::NativeMachineCode;
        }
        if self.events.contains_key("portable_recipe_step") {
            return ExecutionKind::PortableRecipe;
        }
        ExecutionKind::OrdinaryFallback
    }

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
            region_routes: self.region_routes.clone(),
            lowered_route: self.lowered_route.clone(),
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

pub(crate) fn region_route(operations: &'static [crate::ir::Opcode]) {
    update(|profile| {
        let route = operations
            .iter()
            .map(|opcode| opcode.name())
            .collect::<Vec<_>>();
        if !profile.region_routes.contains(&route) {
            profile.region_routes.push(route);
        }
    });
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
#[path = "test_execution_profile_tests.rs"]
mod tests;
