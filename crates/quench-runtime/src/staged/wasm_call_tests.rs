//! The call table of `staged::wasm_call` and the rule of an indirect call's target: the order of
//! its checks on a probe stage, and its verdicts on the interpreter's stage.

use super::wasm_call::{Call, WasmTable, indirect_target};
use super::wasm_flow::Flow;
use super::wasm_machine::ConcreteTable;
use super::wasm_step::Step;
use super::wasm_tests::Probe;
use crate::bytecode::{Op, WideInstruction};
use crate::{Value, WasmTrap};

#[test]
fn the_table_names_each_call_opcode() {
    let named = [
        (Op::CallKnown, Call::Known),
        (Op::WasmIndirectTarget, Call::IndirectTarget),
        (Op::WasmRefFunc, Call::RefFunc),
        (Op::Call, Call::Reference),
    ];
    for (op, call) in named {
        assert_eq!(Call::of(op), Some(call), "{op:?}");
        assert_eq!(Step::of(op, 0), Some(Step::Call(call)), "{op:?}");
    }
    assert_eq!(Call::of(Op::WasmI32Add), None);
}

#[test]
fn only_a_direct_call_names_its_callee_and_a_table_access_needs_callouts() {
    let known = WideInstruction::new(Op::CallKnown, 0, 7, 0, 0);
    assert_eq!(Call::Known.callee(known), Some(7));
    assert_eq!(Step::Call(Call::Known).callee(known), Some(7));
    for call in [Call::IndirectTarget, Call::RefFunc, Call::Reference] {
        assert_eq!(call.callee(known), None);
    }
    assert_eq!(Step::Flow(Flow::Return).callee(known), None);
    let needing = [Call::IndirectTarget, Call::RefFunc];
    for call in [
        Call::Known,
        Call::IndirectTarget,
        Call::RefFunc,
        Call::Reference,
    ] {
        assert_eq!(call.needs_callouts(), needing.contains(&call), "{call:?}");
    }
}

impl Probe {
    fn note(&mut self, entry: &str) -> usize {
        self.log.push(entry.to_owned());
        self.log.len() - 1
    }
}

impl WasmTable for Probe {
    fn table_length(&mut self) -> usize {
        self.note("length")
    }

    fn table_element(&mut self, _: usize) -> usize {
        self.note("element")
    }

    fn reference_is_null(&mut self, _: usize) -> usize {
        self.note("null")
    }

    fn reference_matches(&mut self, _: usize, signature: u32) -> usize {
        self.note(&format!("matches {signature}"))
    }
}

#[test]
fn an_indirect_target_checks_the_index_then_null_then_the_signature() {
    let mut probe = Probe::default();
    indirect_target(&mut probe, 0, 5).expect("the probe never traps");
    assert_eq!(
        probe.log,
        [
            "length",
            "GreaterEqualUnsigned I64",
            "trap UndefinedElement",
            "element",
            "null",
            "trap UninitializedElement",
            "matches 5",
            "EqualZero I32",
            "trap IndirectCallTypeMismatch",
        ]
    );
}

#[test]
fn the_interpreters_stage_traps_in_the_rules_order() {
    let (acceptable, other) = (Value::from_bits(0x10), Value::from_bits(0x20));
    let elements = [Value::NULL, acceptable, other];
    let mut accepts = |reference: Value| reference == acceptable;
    let mut stage = ConcreteTable {
        elements: &elements,
        matches: &mut accepts,
    };
    let expected = [
        (0, Err(WasmTrap::UninitializedElement)),
        (1, Ok(acceptable.to_bits())),
        (2, Err(WasmTrap::IndirectCallTypeMismatch)),
        (3, Err(WasmTrap::UndefinedElement)),
        (u64::MAX, Err(WasmTrap::UndefinedElement)),
    ];
    for (index, verdict) in expected {
        assert_eq!(indirect_target(&mut stage, index, 0), verdict, "{index}");
    }
    assert_eq!(stage.table_length(), 3);
}
