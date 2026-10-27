//! Shared Cranelift target and executable-code backend for JavaScript and Wasm.
//!
//! Frontends retain their own bytecode semantics, but all tiers request a
//! target ISA from this cache and publish code through the same W^X memory
//! implementation in `compiled.rs`.

use cranelift_codegen::settings::{self, Configurable};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

type TargetIsa = dyn cranelift_codegen::isa::TargetIsa;

/// Reuse immutable host target configuration across JS function tiers and
/// Wasm module compilation, one per optimization level.
/// Whether Cranelift verifies the IR of every compile: in debug builds, where tests check
/// each lowering, and not in release, where the verifier cost about a quarter of every JIT
/// compile's instructions (npm semver/browserslist `--jit`, 2026-10-24).
const VERIFY_IR: bool = cfg!(debug_assertions);

pub(super) fn target_isa(opt_level: &str) -> Option<Arc<TargetIsa>> {
    type IsaCache = HashMap<String, Arc<TargetIsa>>;
    static CACHE: OnceLock<Mutex<IsaCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = opt_level.to_owned();
    let mut entries = cache.lock().ok()?;
    if let Some(isa) = entries.get(&key) {
        return Some(Arc::clone(isa));
    }

    let mut flags = settings::builder();
    flags.set("opt_level", opt_level).ok()?;
    flags
        .set("enable_verifier", if VERIFY_IR { "true" } else { "false" })
        .ok()?;
    let isa = cranelift_native::builder()
        .ok()?
        .finish(settings::Flags::new(flags))
        .ok()?;
    entries.insert(key, Arc::clone(&isa));
    Some(isa)
}

#[cfg(test)]
mod tests {
    use cranelift_codegen::{
        Context,
        ir::{self, AbiParam, InstBuilder, MemFlagsData, types},
    };
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

    #[test]
    fn loop_load_reuse_across_branch_split() {
        for intervening_call in [false, true] {
            let isa = super::target_isa("speed").unwrap();
            let mut function = ir::Function::new();
            function.signature.call_conv = isa.default_call_conv();
            function
                .signature
                .params
                .extend([AbiParam::new(types::I64); 2]);
            function.signature.returns.push(AbiParam::new(types::I64));
            let mut frontend = FunctionBuilderContext::new();
            let mut b = FunctionBuilder::new(&mut function, &mut frontend);
            let entry = b.create_block();
            let head = b.create_block();
            let body = b.create_block();
            let exit = b.create_block();
            b.append_block_params_for_function_params(entry);
            b.switch_to_block(entry);
            let address = b.block_params(entry)[0];
            let limit = b.block_params(entry)[1];
            b.ins().jump(head, &[]);
            b.switch_to_block(head);
            let first = b
                .ins()
                .load(types::I64, MemFlagsData::trusted(), address, 0);
            let stop = b.ins().icmp(
                ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
                first,
                limit,
            );
            b.ins().brif(stop, exit, &[], body, &[]);
            b.switch_to_block(body);
            if intervening_call {
                let signature = ir::Signature::new(isa.default_call_conv());
                let signature = b.import_signature(signature);
                b.ins().call_indirect(signature, address, &[]);
            }
            let second = b
                .ins()
                .load(types::I64, MemFlagsData::trusted(), address, 0);
            let next = b.ins().iadd_imm_s(second, 1);
            b.ins().store(MemFlagsData::trusted(), next, address, 0);
            b.ins().jump(head, &[]);
            b.switch_to_block(exit);
            b.ins().return_(&[first]);
            b.seal_all_blocks();
            b.finalize(isa.frontend_config());
            let mut context = Context::for_function(function);
            context
                .optimize(isa.as_ref(), &mut Default::default())
                .unwrap();
            let loads = context
                .func
                .layout
                .blocks()
                .flat_map(|block| context.func.layout.block_insts(block))
                .filter(|&inst| context.func.dfg.insts[inst].opcode() == ir::Opcode::Load)
                .count();
            assert_eq!(
                loads,
                if intervening_call { 2 } else { 1 },
                "{}",
                context.func.display()
            );
        }
    }
}
