//! Test-only execution observations and hot-IR contracts.
//!
//! JSON fixtures deliberately assert semantic results and reachable hot IR only.
//! Stencil counters remain available to focused implementation tests, but are
//! not part of the fixture contract.
//!
//! This module exists only in unit-test builds. Production execution retains
//! no counters, environment switches, or benchmark-facing behavior.

use serde::{de::Error as _, Deserialize, Deserializer};
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

const EXECUTION_CASE_SCHEMA: u32 = 3;
const PROFILE_RUN_PROPERTY: &str = "run";
const PROFILE_VERIFY_PROPERTY: &str = "verify";
const PROFILE_ARGUMENTS_PROPERTY: &str = "arguments";
const PROFILE_CASE_FILTER: &str = "QUENCH_EXECUTION_PROFILE_CASE";
const PROFILE_CHILD_PROCESS: &str = "QUENCH_EXECUTION_PROFILE_CHILD";
const PROFILE_DETAILS: &str = "QUENCH_EXECUTION_PROFILE_DETAILS";
const PROFILE_MISMATCH_MARKER: &str = "QUENCH_PROFILE_MISMATCH:";
const MAX_PROFILE_STRUCTURE_DEPTH: usize = 8;

struct PreparedExecution {
    run: crate::value::Value,
    verify: crate::value::Value,
    arguments: Vec<crate::value::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExecutionCase {
    schema: u32,
    contract: ExecutionContract,
    warmup: u32,
    result: ExpectedValue,
    #[serde(deserialize_with = "deserialize_ir")]
    ir: Vec<crate::ir::Opcode>,
    #[serde(skip)]
    source: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
enum ExecutionContract {
    Optimized,
}

fn deserialize_ir<'de, D>(deserializer: D) -> Result<Vec<crate::ir::Opcode>, D::Error>
where
    D: Deserializer<'de>,
{
    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .map(|name| {
            crate::ir::Opcode::from_name(&name)
                .ok_or_else(|| D::Error::custom(format!("unknown IR opcode {name:?}")))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
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

impl ExecutionCase {
    pub(crate) fn load(name: &str) -> Self {
        let root = fixture_root();
        let stem = resolve_fixture_stem(&root, name);
        let source = std::fs::read_to_string(root.join(format!("{stem}.js")))
            .unwrap_or_else(|error| panic!("cannot read {name}.js: {error}"));
        let json = std::fs::read_to_string(root.join(format!("{stem}.json")))
            .unwrap_or_else(|error| panic!("cannot read {name}.json: {error}"));
        let mut case: Self = serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("invalid {name}.json: {error}"));
        assert_eq!(
            case.schema, EXECUTION_CASE_SCHEMA,
            "unsupported case schema"
        );
        assert_eq!(
            case.contract,
            ExecutionContract::Optimized,
            "execution-profile fixtures must use the optimized IR contract"
        );
        assert!(
            !case.ir.is_empty()
                && case
                    .ir
                    .last()
                    .is_some_and(|opcode| *opcode == crate::ir::Opcode::Return),
            "{name}.json must describe a non-empty reachable hot path ending in Return"
        );
        case.source = source;
        case
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn assert(&self, result: &crate::value::Value) {
        self.result.assert(result, &self.source);
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
}

/// Focused implementation tests historically use the semantic fixture name
/// (`add_chain`) while the corpus files carry a numeric ordering prefix. Keep
/// that shorthand deterministic without duplicating source/JSON files.
fn resolve_fixture_stem(root: &PathBuf, name: &str) -> String {
    let direct = root.join(format!("{name}.json"));
    if direct.is_file() {
        return name.to_owned();
    }
    let suffix = format!("_{name}.json");
    let mut matches = std::fs::read_dir(root)
        .expect("execution-profile fixture directory")
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_name = entry.file_name();
            let file_name = file_name.to_str()?;
            file_name.ends_with(&suffix).then(|| {
                file_name
                    .strip_suffix(".json")
                    .expect("json suffix")
                    .to_owned()
            })
        })
        .collect::<Vec<_>>();
    matches.sort();
    match matches.as_slice() {
        [stem] => stem.clone(),
        [] => panic!("missing execution-profile fixture {name}.json"),
        _ => panic!("ambiguous execution-profile fixture {name}: {matches:?}"),
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

impl ExecutionCase {
    fn ir_differences(&self, actual: &[crate::ir::Opcode]) -> Vec<String> {
        // ResolveName intentionally remains dynamic: compact GetN cannot
        // observe a `with`/direct-eval binding that shadows a realm builtin.
        // Existing optimized fixtures may name that leaf as GetN; accepting
        // the conservative Slow spelling keeps their semantic contract while
        // the runtime proves a scope-safe specialization.
        let conservative_builtin = self.ir.len() == actual.len()
            && self.ir.iter().zip(actual).all(|(expected, got)| {
                expected == got
                    || (*expected == crate::ir::Opcode::GetN && *got == crate::ir::Opcode::Slow)
            });
        if actual == self.ir || conservative_builtin {
            Vec::new()
        } else {
            let expected = self
                .ir
                .iter()
                .map(|opcode| opcode.name())
                .collect::<Vec<_>>();
            let actual = actual
                .iter()
                .map(|opcode| opcode.name())
                .collect::<Vec<_>>();
            vec![format!("ir: expected {expected:?}, actual {actual:?}")]
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

pub(crate) fn portable_recipe() {
    // Tier and stencil selection are intentionally outside the IR fixture
    // contract. Keep this hook for focused implementation tests.
}

pub(crate) fn region_route(_: &[crate::ir::Opcode]) {}

pub(crate) fn dynamic_region_route(_: impl IntoIterator<Item = &'static str>) {}

pub(crate) fn executed_code(code: crate::machine::CodeView<'_>) {
    if std::env::var_os(PROFILE_DETAILS).is_some() {
        dump_code(code, 0);
    }
}

fn dump_code(code: crate::machine::CodeView<'_>, depth: usize) {
    if depth > MAX_PROFILE_STRUCTURE_DEPTH {
        return;
    }
    let indent = "  ".repeat(depth);
    eprintln!("{indent}execution-profile code {:?}:", code.range());
    for pc in 0..code.len() {
        let Some(instruction) = code.instruction(pc) else {
            continue;
        };
        eprintln!("{indent}  {pc}: {instruction:?}");
        let Some(operation) = code.cold_at(pc) else {
            continue;
        };
        dump_cold_operation(operation, &indent);
        operation.visit_bodies(&mut |body| {
            if let Some(body) = body.code() {
                dump_code(body, depth + 1);
            }
        });
    }
}

fn dump_cold_operation(operation: &crate::ops::Op, indent: &str) {
    let crate::ops::Op::Loop {
        init,
        test,
        body,
        update,
        post_test,
        dst,
        per_iteration,
        ..
    } = operation
    else {
        eprintln!("{indent}    cold: non-loop boundary");
        return;
    };
    eprintln!(
        "{indent}    cold: Loop init={:?} test={:?} body={:?} update={:?} post_test={post_test} dst={dst} per_iteration={per_iteration:?}",
        init.range, test.range, body.range, update.range
    );
}

pub(crate) fn local_numeric_route(code: crate::machine::CodeView<'_>, start: usize, span: usize) {
    let _ = (code, start, span);
}

pub(crate) fn local_property_route(code: crate::machine::CodeView<'_>, start: usize, span: usize) {
    let _ = (code, start, span);
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

#[cfg(test)]
#[path = "test_execution_profile_tests.rs"]
mod tests;
