//! The public native function entry of a function: a trampoline that runs the function's body
//! (its lane body where there is one, else its base body at PC 0) and maps its completion to the
//! entry's status. The body continues the activation itself (a lane leave tail-jumps to the base
//! body, `aot_image::join`), so the entry runs no per-PC loop. The body's receiver and dense
//! captures are the payload's, bound once per activation (`JsStepDispatch::bind_captures`).
use super::*;
use super::lane_region::DISPATCH;
use crate::staged::frame::{CONTEXT_BODY, CONTEXT_THIS};
use crate::staged::{flow::FlowStage, NumericStage, Stage};

/// The positions of the entry ABI's words the trampoline reads (`native_entry_signature`):
/// the context is the first, the arguments pointer and the result word address the last two.
const ENTRY_CONTEXT: usize = 0;
const ENTRY_ARGUMENTS: usize = 3;
const ENTRY_RESULT: usize = 4;

/// Where a trampoline reaches the body it enters, a body of the lane body ABI, as the call a
/// publication binds (an address, or a symbol).
pub(in crate::vm) struct AheadTargets<'a> {
    /// The body's call on the body ABI's words, giving its status and result word.
    pub(in crate::vm) body: &'a mut dyn FnMut(&mut FunctionBuilder<'_>, &[ir::Value]) -> (ir::Value, ir::Value),
}

/// The entry of function `function_id` over `targets`, named by the publication's own symbol
/// space `namespace`.
pub(in crate::vm) fn lower_ahead_entry(
    namespace: u32,
    function_id: u32,
    isa: &dyn cranelift_codegen::isa::TargetIsa,
    targets: AheadTargets<'_>,
) -> Result<ir::Function, String> {
    let mut native = ir::Function::with_name_signature(
        ir::UserFuncName::user(namespace, function_id),
        native_entry_signature(isa.pointer_type(), isa.default_call_conv()),
    );
    regions::declare_regions(&mut native);
    let mut context = crate::vm::jit_backend::builder::take_context();
    let mut builder = FunctionBuilder::new(&mut native, &mut context);
    let (_, params) = crate::vm::jit_backend::builder::enter(&mut builder);
    emit_body(&mut builder, &params, targets.body);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    crate::vm::jit_backend::builder::keep_context(context);
    Ok(native)
}

/// Calls the body on the activation's window, whose header holds the receiver and captures
/// (`CONTEXT_THIS`, `CONTEXT_BODY`), which the entry's caller wrote.
fn emit_body(
    builder: &mut FunctionBuilder<'_>,
    params: &[ir::Value],
    body: &mut dyn FnMut(&mut FunctionBuilder<'_>, &[ir::Value]) -> (ir::Value, ir::Value),
) {
    let window = crate::staged::Region::Frame.mutable();
    let (this, captures) = {
        let mut stage = super::object_slots::emit(builder);
        let this = stage.load_u64(window, params[ENTRY_ARGUMENTS], -(8 * CONTEXT_THIS as i32));
        let captures = stage.load_u64(window, params[ENTRY_ARGUMENTS], -(8 * CONTEXT_BODY as i32));
        (this, captures)
    };
    let (status, word) = body(builder, &[params[ENTRY_CONTEXT], this, captures, params[ENTRY_ARGUMENTS]]);
    emit_body_status(builder, params, (status, word));
}

/// A completion stores its value and returns its record's status; any other status is the
/// entry's own and is returned as it is.
fn emit_body_status(builder: &mut FunctionBuilder<'_>, params: &[ir::Value], (status, word): (ir::Value, ir::Value)) {
    let returned = builder.create_block();
    let mut stage = super::object_slots::emit(builder);
    let wide = stage.uextend32(status);
    let running = stage.ne_imm(wide, u64::from(LANE_BODY_RETURNED));
    unforced(|| stage.guard(running, returned));
    stage.ret(&[status]);
    stage.builder.switch_to_block(returned);
    stage.store_u64(DISPATCH, params[ENTRY_RESULT], 0, word);
    let payload = stage.load_u64(DISPATCH, params[ENTRY_CONTEXT], std::mem::offset_of!(CallContext, head.adapter_payload) as i32);
    let record = emit_js_payload_word(stage.builder, payload, std::mem::offset_of!(JsStepDispatch, head.returned_pc));
    let record = stage.and_imm(record, u64::from(u32::MAX));
    let wide = stage.add_imm(record, u64::from(RETURN_STATUS_BASE));
    let status = stage.narrow(wide);
    stage.ret(&[status]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trampoline over any body names no instruction of its own: its blocks are the window's
    /// header loads and the status map, and it verifies as one function.
    #[test]
    fn a_trampoline_verifies_over_any_body() {
        let isa = crate::vm::jit_backend::target_isa().unwrap();
        let function = lower_ahead_entry(0, 1, &*isa, AheadTargets {
            body: &mut |builder, words| {
                let target = object_slots::emit(builder).constant(8);
                let word = emit_lane_call(builder, target, words);
                (object_slots::emit(builder).narrow(word), word)
            },
        }).unwrap();
        cranelift_codegen::verify_function(&function, &*isa).unwrap();
    }
}

/// The shrink-only budget of generic-row records the AOT image leaves to the generic semantic
/// row in the benchmark sources (`aot-exact-steps.txt`): a source whose count grows fails the
/// check, and one whose count shrinks must lower its budget, as `oversized.txt` ratchets file sizes.
#[cfg(all(test, feature = "aot"))]
mod exact_steps {
    use super::*;

    const BUDGET: &str = include_str!("../../../aot-exact-steps.txt");

    /// The generic-row records of the AOT image of `source`.
    fn generic_rows(source: &str, name: &str) -> usize {
        let program = crate::Engine::specialize(source, name).expect("benchmark source compiles");
        let facts = crate::compile::aot_facts::analyze(&program);
        let target = crate::vm::aot_image::ImageTarget { isa: crate::vm::jit_backend::IsaBaseline::default() };
        crate::vm::aot_image::lower(&program, &facts, target).expect("benchmark source lowers ahead").generic_rows()
    }

    #[test]
    fn aot_exact_steps_stay_within_budget() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut failures = Vec::new();
        for line in BUDGET.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
            let (budget, path) = line.split_once(' ').expect("`count path` budget line");
            let budget: usize = budget.parse().expect("budget count");
            let source = std::fs::read_to_string(root.join(path)).expect("budgeted benchmark source");
            let count = generic_rows(&source, path);
            if count != budget {
                failures.push(format!("{path}: {count} generic-row records, budget {budget}"));
            }
        }
        assert!(failures.is_empty(), "generic-row records left the budget (lower it when they shrink; \
            lower an instruction natively when they grow):\n{}", failures.join("\n"));
    }
}
