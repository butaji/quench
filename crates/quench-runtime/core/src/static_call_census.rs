use crate::dynbytecode::{DynCode, DynOp, Register};
use crate::inline_plan::InitialInlineDecision;
use crate::{Env, Environment, FunctionKind, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

const EMPTY_COUNTER: u64 = 0;

struct StaticCallCensus {
    call_sites: AtomicU64,
    direct_binding_sites: AtomicU64,
    missing_binding_sites: AtomicU64,
    user_function_targets: AtomicU64,
    compatible_user_function_targets: AtomicU64,
    builtin_targets: AtomicU64,
    native_targets: AtomicU64,
    non_function_targets: AtomicU64,
    computed_target_sites: AtomicU64,
    exact_user_target_executions: AtomicU64,
    exact_compatible_user_target_executions: AtomicU64,
    exact_eligible_user_target_executions: AtomicU64,
    exact_compatible_eligible_user_target_executions: AtomicU64,
    changed_user_target_executions: AtomicU64,
}

static STATIC_CALL_CENSUS: StaticCallCensus = StaticCallCensus {
    call_sites: AtomicU64::new(EMPTY_COUNTER),
    direct_binding_sites: AtomicU64::new(EMPTY_COUNTER),
    missing_binding_sites: AtomicU64::new(EMPTY_COUNTER),
    user_function_targets: AtomicU64::new(EMPTY_COUNTER),
    compatible_user_function_targets: AtomicU64::new(EMPTY_COUNTER),
    builtin_targets: AtomicU64::new(EMPTY_COUNTER),
    native_targets: AtomicU64::new(EMPTY_COUNTER),
    non_function_targets: AtomicU64::new(EMPTY_COUNTER),
    computed_target_sites: AtomicU64::new(EMPTY_COUNTER),
    exact_user_target_executions: AtomicU64::new(EMPTY_COUNTER),
    exact_compatible_user_target_executions: AtomicU64::new(EMPTY_COUNTER),
    exact_eligible_user_target_executions: AtomicU64::new(EMPTY_COUNTER),
    exact_compatible_eligible_user_target_executions: AtomicU64::new(EMPTY_COUNTER),
    changed_user_target_executions: AtomicU64::new(EMPTY_COUNTER),
};

type SiteKey = (usize, u32);

static STATIC_RESOLUTIONS: LazyLock<Mutex<HashMap<SiteKey, StaticCallResolution>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StaticCallResolution {
    Computed,
    MissingBinding,
    UserFunction {
        identity_bits: u64,
        environment_compatible: bool,
    },
    Builtin,
    Native,
    NonFunction,
}

#[derive(Clone)]
enum RegisterOrigin {
    DirectBinding(String),
}

pub(crate) fn record(code: &DynCode, outer: &Env) {
    let mut origins = vec![None; code.registers];
    let block_starts = block_start_table(code);

    for (pc, instruction) in code.ops.iter().enumerate() {
        if block_starts[pc] {
            origins.fill(None);
        }
        if let DynOp::Call { callee, .. } = &instruction.op {
            let resolution = record_call(origin(&origins, *callee), outer);
            publish_resolution(code.source_id, instruction.span.start, resolution);
        }
        update_origin(&mut origins, &instruction.op);
    }
}

fn block_start_table(code: &DynCode) -> Vec<bool> {
    let mut starts = vec![false; code.ops.len()];
    if let Some(entry) = starts.first_mut() {
        *entry = true;
    }
    for (start, _, _) in &code.blocks {
        if let Some(entry) = starts.get_mut(*start) {
            *entry = true;
        }
    }
    starts
}

fn origin(origins: &[Option<RegisterOrigin>], register: Register) -> Option<&RegisterOrigin> {
    origins.get(usize::from(register))?.as_ref()
}

fn update_origin(origins: &mut [Option<RegisterOrigin>], op: &DynOp) {
    let Some(destination) = op.written_register().map(usize::from) else {
        return;
    };
    let next = match op {
        DynOp::LoadName { name, .. } => Some(RegisterOrigin::DirectBinding(name.clone())),
        DynOp::Move { src, .. } => origins.get(usize::from(*src)).cloned().flatten(),
        _ => None,
    };
    if let Some(slot) = origins.get_mut(destination) {
        *slot = next;
    }
}

fn record_call(origin: Option<&RegisterOrigin>, outer: &Env) -> StaticCallResolution {
    STATIC_CALL_CENSUS
        .call_sites
        .fetch_add(1, Ordering::Relaxed);
    let Some(RegisterOrigin::DirectBinding(name)) = origin else {
        STATIC_CALL_CENSUS
            .computed_target_sites
            .fetch_add(1, Ordering::Relaxed);
        return StaticCallResolution::Computed;
    };
    STATIC_CALL_CENSUS
        .direct_binding_sites
        .fetch_add(1, Ordering::Relaxed);
    let Some(value) = Environment::get(outer, name) else {
        STATIC_CALL_CENSUS
            .missing_binding_sites
            .fetch_add(1, Ordering::Relaxed);
        return StaticCallResolution::MissingBinding;
    };
    let Some(function) = value.as_function_ref() else {
        STATIC_CALL_CENSUS
            .non_function_targets
            .fetch_add(1, Ordering::Relaxed);
        return StaticCallResolution::NonFunction;
    };
    let (counter, resolution) = match &function.kind {
        FunctionKind::User {
            env: target_outer, ..
        } => {
            let environment_compatible = std::rc::Rc::ptr_eq(outer, target_outer);
            if environment_compatible {
                STATIC_CALL_CENSUS
                    .compatible_user_function_targets
                    .fetch_add(1, Ordering::Relaxed);
            }
            (
                &STATIC_CALL_CENSUS.user_function_targets,
                StaticCallResolution::UserFunction {
                    identity_bits: value.as_borrowed_raw().bits(),
                    environment_compatible,
                },
            )
        }
        FunctionKind::Arrow {
            env: target_outer, ..
        } => {
            let environment_compatible = std::rc::Rc::ptr_eq(outer, target_outer);
            (
                &STATIC_CALL_CENSUS.user_function_targets,
                StaticCallResolution::UserFunction {
                    identity_bits: value.as_borrowed_raw().bits(),
                    environment_compatible,
                },
            )
        }
        FunctionKind::Builtin(_) => (
            &STATIC_CALL_CENSUS.builtin_targets,
            StaticCallResolution::Builtin,
        ),
        FunctionKind::Native(_) => (
            &STATIC_CALL_CENSUS.native_targets,
            StaticCallResolution::Native,
        ),
    };
    counter.fetch_add(1, Ordering::Relaxed);
    resolution
}

fn publish_resolution(source_id: Option<usize>, span_start: u32, resolution: StaticCallResolution) {
    let Some(source_id) = source_id else {
        return;
    };
    STATIC_RESOLUTIONS
        .lock()
        .expect("static call resolution table is not poisoned")
        .insert((source_id, span_start), resolution);
}

pub(crate) fn resolution(source_id: Option<usize>, span_start: u32) -> StaticCallResolution {
    let Some(source_id) = source_id else {
        return StaticCallResolution::Computed;
    };
    STATIC_RESOLUTIONS
        .lock()
        .expect("static call resolution table is not poisoned")
        .get(&(source_id, span_start))
        .copied()
        .unwrap_or(StaticCallResolution::Computed)
}

pub(crate) fn record_user_execution(
    resolution: StaticCallResolution,
    actual: &Value,
    decision: InitialInlineDecision,
) {
    let StaticCallResolution::UserFunction {
        identity_bits,
        environment_compatible,
    } = resolution
    else {
        return;
    };
    if actual.as_borrowed_raw().bits() != identity_bits {
        STATIC_CALL_CENSUS
            .changed_user_target_executions
            .fetch_add(1, Ordering::Relaxed);
        return;
    }
    STATIC_CALL_CENSUS
        .exact_user_target_executions
        .fetch_add(1, Ordering::Relaxed);
    if environment_compatible {
        STATIC_CALL_CENSUS
            .exact_compatible_user_target_executions
            .fetch_add(1, Ordering::Relaxed);
    }
    if decision == InitialInlineDecision::Eligible {
        STATIC_CALL_CENSUS
            .exact_eligible_user_target_executions
            .fetch_add(1, Ordering::Relaxed);
        if environment_compatible {
            STATIC_CALL_CENSUS
                .exact_compatible_eligible_user_target_executions
                .fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub(crate) fn stats_json_fields() -> String {
    let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
    format!(
        concat!(
            ",\"static_call_sites\":{},\"static_direct_binding_sites\":{},",
            "\"static_missing_binding_sites\":{},\"static_user_function_targets\":{},",
            "\"static_compatible_user_function_targets\":{},",
            "\"static_builtin_targets\":{},\"static_native_targets\":{},",
            "\"static_non_function_targets\":{},\"static_computed_target_sites\":{},",
            "\"static_exact_user_target_executions\":{},",
            "\"static_exact_compatible_user_target_executions\":{},",
            "\"static_exact_eligible_user_target_executions\":{},",
            "\"static_exact_compatible_eligible_user_target_executions\":{},",
            "\"static_changed_user_target_executions\":{}"
        ),
        load(&STATIC_CALL_CENSUS.call_sites),
        load(&STATIC_CALL_CENSUS.direct_binding_sites),
        load(&STATIC_CALL_CENSUS.missing_binding_sites),
        load(&STATIC_CALL_CENSUS.user_function_targets),
        load(&STATIC_CALL_CENSUS.compatible_user_function_targets),
        load(&STATIC_CALL_CENSUS.builtin_targets),
        load(&STATIC_CALL_CENSUS.native_targets),
        load(&STATIC_CALL_CENSUS.non_function_targets),
        load(&STATIC_CALL_CENSUS.computed_target_sites),
        load(&STATIC_CALL_CENSUS.exact_user_target_executions),
        load(&STATIC_CALL_CENSUS.exact_compatible_user_target_executions),
        load(&STATIC_CALL_CENSUS.exact_eligible_user_target_executions),
        load(&STATIC_CALL_CENSUS.exact_compatible_eligible_user_target_executions),
        load(&STATIC_CALL_CENSUS.changed_user_target_executions),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynbytecode::{DynInstr, Literal};
    use oxc_span::Span;

    const CALLEE_REGISTER: Register = 0;
    const RESULT_REGISTER: Register = 1;
    const GLOBAL_FUNCTION_NAME: &str = "helper";

    #[test]
    fn direct_binding_survives_moves_but_not_block_boundaries() {
        let mut origins = vec![None; usize::from(RESULT_REGISTER) + 1];
        update_origin(
            &mut origins,
            &DynOp::LoadName {
                dst: CALLEE_REGISTER,
                name: GLOBAL_FUNCTION_NAME.into(),
            },
        );
        update_origin(
            &mut origins,
            &DynOp::Move {
                dst: RESULT_REGISTER,
                src: CALLEE_REGISTER,
            },
        );
        assert!(matches!(
            origin(&origins, RESULT_REGISTER),
            Some(RegisterOrigin::DirectBinding(name)) if name == GLOBAL_FUNCTION_NAME
        ));

        origins.fill(None);
        assert!(origin(&origins, RESULT_REGISTER).is_none());
    }

    #[test]
    fn non_binding_write_clears_previous_origin() {
        let mut origins = vec![Some(RegisterOrigin::DirectBinding(
            GLOBAL_FUNCTION_NAME.into(),
        ))];
        let instruction = DynInstr {
            op: DynOp::LoadLiteral {
                dst: CALLEE_REGISTER,
                value: Literal::Undefined,
            },
            span: Span::default(),
        };
        update_origin(&mut origins, &instruction.op);
        assert!(origin(&origins, CALLEE_REGISTER).is_none());
    }
}
