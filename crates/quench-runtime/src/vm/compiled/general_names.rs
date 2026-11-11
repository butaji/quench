//! Global name reads in general dispatch code: a `LoadName` of a name the global object holds
//! as a cached data property, answered by `general_global_get` without the exact step, as the
//! step's own cache probe answers it. A lexical declaration, an accessor or a miss takes the
//! step at the same PC.
use super::{emit_js_payload_word, emit_lane_helper_call, Host, JsExactFallback, JsStepDispatch, ResidualProgram, Vm};
use crate::bytecode::WideInstruction;
use crate::Value;
use cranelift_codegen::ir::{self, condcodes::IntCC, types, InstBuilder, MemFlagsData};
use cranelift_frontend::FunctionBuilder;

/// Lowers the global read at the exact fallback's PC; false where its result or callee
/// register has no home.
pub(super) fn lower_global_read(
    builder: &mut FunctionBuilder<'_>,
    (instruction, next): (WideInstruction, ir::Block),
    register_offset: &dyn Fn(u16) -> Option<i32>,
    (arguments, payload): (ir::Value, ir::Value),
    exact: &JsExactFallback,
) -> bool {
    let Some(destination) = register_offset(instruction.result_register()) else { return false };
    // A call's receiver register reads `undefined` (`LoadNameCall`).
    let receiver = if instruction.op().is_load_name_call() {
        let Some(receiver) = register_offset(instruction.register_b()) else { return false };
        Some(receiver)
    } else { None };
    let read = (u64::from(instruction.atom_index()) << u16::BITS) | u64::from(instruction.cache_site_index());
    let helper = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, global_get));
    let read = builder.ins().iconst(types::I64, read as i64);
    let value = emit_lane_helper_call(builder, payload, helper, &[payload, read]);
    let hit = builder.ins().icmp_imm_u(IntCC::NotEqual, value, Value::DELETED.to_bits() as i64);
    let (done, fallback) = (builder.create_block(), builder.create_block());
    builder.ins().brif(hit, done, &[], fallback, &[]);
    builder.switch_to_block(done);
    builder.ins().store(MemFlagsData::trusted(), value, arguments, destination);
    if let Some(receiver) = receiver {
        let undefined = builder.ins().iconst(types::I64, Value::UNDEFINED.to_bits() as i64);
        builder.ins().store(MemFlagsData::trusted(), undefined, arguments, receiver);
    }
    builder.ins().jump(next, &[]);
    exact.emit(builder, fallback);
    true
}

/// The global object's cached data property `read` names (its atom above its cache site),
/// unless a global lexical declaration binds the name; `Value::DELETED` for a miss.
///
/// # Safety
/// `vm`, `residual` and `payload` are the running VM, residual and payload, live and unborrowed for the call.
pub(super) unsafe extern "C" fn general_global_get<H: Host>(vm: u64, residual: u64, payload: u64, read: u64) -> u64 {
    let (atom, site) = ((read >> u16::BITS) as crate::bytecode::Atom, read as u16);
    // SAFETY: as the caller promises; the probe reads caches and never allocates.
    let (vm, residual, payload) = unsafe {
        (&mut *(vm as *mut Vm<H>), &*(residual as *const ResidualProgram), &mut *(payload as *mut JsStepDispatch))
    };
    if !payload.names_global || vm.realm.global_lexical_declarations.contains(&atom) { return Value::DELETED.to_bits(); }
    match vm.cached_field_hit(residual, vm.realm.globals, site) {
        super::super::object::FieldCacheProbe::Hit(value) => {
            #[cfg(test)] { payload.global_name_cache_hits += 1; }
            value.to_bits()
        }
        _ => Value::DELETED.to_bits(),
    }
}
