//! The call table of `staged::wasm_call` and the rule of an indirect call's target: the order of
//! its checks on a probe stage, and its verdicts on the interpreter's stage.

use super::machine::MachineStage;
use super::wasm::Width;
use super::wasm_call::{Call, WasmTable, indirect_target};
use super::wasm_flow::Flow;
use super::wasm_machine::ConcreteTable;
use super::wasm_step::Step;
use super::wasm_tests::Probe;
use crate::bytecode::{Op, WideInstruction};
use crate::heap::TableTarget;
use crate::wasm::{CanonicalSignature, WasmSignatures};
use crate::{
    Value, WasmCallableType, WasmSignature, WasmTrap, WasmType, WasmTypes,
};

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
    fn table_range_exceeds(&mut self, _: usize, _: usize) -> usize {
        self.note("range")
    }

    fn element_signature(&mut self, _: usize) -> usize {
        self.note("signature")
    }

    fn table_element(&mut self, _: usize) -> usize {
        self.note("element")
    }

    type Row = usize;

    fn element_row(&mut self, _: usize) -> usize {
        self.note("row")
    }

    fn signature_accepts(&mut self, _: usize, _: usize, _: usize) -> usize {
        self.note("accepts")
    }

    fn unless_equal(
        &mut self,
        _: usize,
        _: usize,
        otherwise: impl FnOnce(&mut Self) -> Result<usize, Self::Exit>,
    ) -> Result<usize, Self::Exit> {
        self.note("unless equal");
        otherwise(self)
    }
}

/// The signatures of a probed table's functions and of a call: a function, another, a host's.
fn ids() -> [CanonicalSignature; 3] {
    let pool = |function: WasmCallableType| {
        WasmSignatures::with_imports("ids", &[], [function], &WasmTypes::default()).unwrap()
    };
    let signature = |result: WasmType| {
        WasmCallableType::Embedded(WasmSignature {
            params: vec![],
            results: vec![result],
        })
    };
    let first = pool(signature(WasmType::I32)).defined_canonical(0).unwrap();
    let second = pool(signature(WasmType::I64)).defined_canonical(0).unwrap();
    [first, second, CanonicalSignature::FOREIGN]
}

#[test]
fn an_indirect_target_checks_the_index_then_an_unequal_signature_for_null_then_relates() {
    let mut probe = Probe::default();
    let [expected, ..] = ids();
    indirect_target(&mut probe, 0, expected).expect("the probe never traps");
    assert_eq!(
        probe.log,
        [
            "constant I64 1".to_owned(),
            "range".to_owned(),
            "trap UndefinedElement".to_owned(),
            "signature".to_owned(),
            "element".to_owned(),
            format!("constant I64 {}", expected.word()),
            "unless equal".to_owned(),
            format!("constant I64 {}", CanonicalSignature::NULL.word()),
            "Equal I64".to_owned(),
            "trap UninitializedElement".to_owned(),
            "accepts".to_owned(),
            "EqualZero I32".to_owned(),
            "trap IndirectCallTypeMismatch".to_owned(),
            "row".to_owned(),
        ]
    );
}

#[test]
fn the_interpreters_stage_traps_in_the_rules_order() {
    let [expected, other, foreign] = ids();
    let (acceptable, unrelated) = (Value::from_bits(0x10), Value::from_bits(0x20));
    let elements = [Value::NULL, acceptable, unrelated, acceptable];
    let targets = [CanonicalSignature::NULL, expected, other, foreign]
        .map(|signature| TableTarget { signature, ..TableTarget::FOREIGN });
    let mut related = |reference: Value| reference == acceptable;
    let mut stage = ConcreteTable {
        elements: &elements,
        targets: &targets,
        related: &mut related,
    };
    let verdicts = [
        (0, Err(WasmTrap::UninitializedElement)),
        (1, Ok(acceptable.to_bits())),
        (2, Err(WasmTrap::IndirectCallTypeMismatch)),
        (3, Ok(acceptable.to_bits())),
        (4, Err(WasmTrap::UndefinedElement)),
        (u64::MAX, Err(WasmTrap::UndefinedElement)),
    ];
    for (index, verdict) in verdicts {
        let result = indirect_target(&mut stage, index, expected).map(|(reference, _)| reference);
        assert_eq!(result, verdict, "{index}");
    }
    let one = stage.constant(Width::I64, 1);
    assert_eq!(stage.table_range_exceeds(3, one), 0);
    assert_eq!(stage.table_range_exceeds(4, one), 1);
}
