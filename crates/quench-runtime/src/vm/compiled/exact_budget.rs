//! How general dispatch lowers each PC, and the shrink-only budget of exact-only PCs in
//! the benchmark sources AOT compiles (`aot-exact-steps.txt`): a source whose count grows
//! fails the check, and one whose count shrinks must lower its budget, as `oversized.txt`
//! ratchets file sizes.

/// The lowering of one PC, one byte per PC of a general dispatch artifact.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub(in crate::vm) enum PcLowering {
    /// Only the exact step (`js_semantic_callout`) runs the instruction: a VM hand-back.
    Exact = 0,
    /// Native code runs the instruction, its misses taking the exact step.
    Native = 1,
    /// Native code runs the instruction, but an exact step that reaches it in a batch
    /// steps it in place rather than returning to native code (calls).
    Stepped = 2,
}

// The budget reads AOT lowering (`JsGeneralCallout::Address`), which only the aot feature builds.
#[cfg(all(test, feature = "aot"))]
mod tests {
    use super::PcLowering;
    use std::collections::BTreeMap;
    use crate::vm::compiled::{lower_js_general_dispatch, JsGeneralCallout};

    const BUDGET: &str = include_str!("../../../aot-exact-steps.txt");

    /// The exact-only PCs of every function general dispatch lowers in `source`, by
    /// opcode; a function it rejects counts all its PCs. The root body runs once and is not
    /// counted.
    fn exact_steps(source: &str, name: &str) -> BTreeMap<&'static str, usize> {
        let program = crate::Engine::specialize(source, name).expect("benchmark source compiles");
        let isa = crate::vm::jit_backend::target_isa().expect("host ISA");
        let mut ops = BTreeMap::new();
        for id in 1..program.function_count() {
            let function = program.function(id);
            let lowered = lower_js_general_dispatch(&program, id as u32, &*isa, JsGeneralCallout::Address(0), &[], &[]);
            let pcs = lowered.map_or_else(|_| vec![PcLowering::Exact as u8; function.code.len()], |(_, pcs)| pcs);
            for (packed, _) in function.code.iter().zip(pcs).filter(|(_, lowering)| *lowering == PcLowering::Exact as u8) {
                let op = crate::vm::dispatch_fast::decoded_instruction(function, *packed).op();
                *ops.entry(crate::bytecode::Op::NAMES[op as usize]).or_default() += 1;
            }
        }
        ops
    }

    #[test]
    fn aot_exact_steps_stay_within_budget() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let mut failures = Vec::new();
        for line in BUDGET.lines().filter(|line| !line.is_empty() && !line.starts_with('#')) {
            let (budget, path) = line.split_once(' ').expect("`count path` budget line");
            let budget: usize = budget.parse().expect("budget count");
            let source = std::fs::read_to_string(root.join(path)).expect("budgeted benchmark source");
            let ops = exact_steps(&source, path);
            let count: usize = ops.values().sum();
            if count != budget {
                failures.push(format!("{path}: {count} exact-only PCs, budget {budget}: {ops:?}"));
            }
        }
        assert!(failures.is_empty(), "exact-only PCs left the budget (lower it when they shrink; \
            lower an instruction natively when they grow):\n{}", failures.join("\n"));
    }
}
