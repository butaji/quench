use super::analysis::*;
use super::guard::GuardPlan;
use super::model::*;
use crate::dynbytecode::{DynCode, DynInstr, DynOp, Literal, Register};
use crate::{Object, Op, Value, test_object};
use oxc_span::Span;

const LOOP_START: usize = 0;
const LOOP_EXIT: usize = 13;
const OBJECT_LOCAL: usize = 0;
const INDEX_LOCAL: usize = 1;
const BOUND_LOCAL: usize = 2;
const OBJECT_REGISTER: Register = 0;
const INDEX_REGISTER: Register = 1;
const BOUND_REGISTER: Register = 2;
const CONDITION_REGISTER: Register = 3;
const ELEMENT_REGISTER: Register = 4;
const ADDEND_REGISTER: Register = 5;
const RESULT_REGISTER: Register = 6;
const ONE_REGISTER: Register = 7;
const NEXT_INDEX_REGISTER: Register = 8;
const ONE: f64 = 1.0;

fn instruction(op: DynOp) -> DynInstr {
    DynInstr {
        op,
        span: Span::default(),
    }
}

fn loop_code(tail: Vec<DynOp>) -> DynCode {
    let mut ops = vec![
        DynOp::LoadLocal {
            dst: OBJECT_REGISTER,
            slot: OBJECT_LOCAL,
        },
        DynOp::LoadLocal {
            dst: INDEX_REGISTER,
            slot: INDEX_LOCAL,
        },
        DynOp::LoadLocal {
            dst: BOUND_REGISTER,
            slot: BOUND_LOCAL,
        },
        DynOp::Binary {
            dst: CONDITION_REGISTER,
            left: INDEX_REGISTER,
            right: BOUND_REGISTER,
            kind: Op::Lt,
        },
        DynOp::JumpIfFalse {
            test: CONDITION_REGISTER,
            target: LOOP_EXIT,
        },
        DynOp::GetComputed {
            dst: ELEMENT_REGISTER,
            object: OBJECT_REGISTER,
            key: INDEX_REGISTER,
        },
        DynOp::LoadLocal {
            dst: ADDEND_REGISTER,
            slot: BOUND_LOCAL,
        },
        DynOp::Binary {
            dst: RESULT_REGISTER,
            left: ELEMENT_REGISTER,
            right: ADDEND_REGISTER,
            kind: Op::Add,
        },
        DynOp::SetComputed {
            object: OBJECT_REGISTER,
            key: INDEX_REGISTER,
            src: RESULT_REGISTER,
            accessor: None,
        },
        DynOp::LoadLiteral {
            dst: ONE_REGISTER,
            value: Literal::Number(ONE),
        },
        DynOp::Binary {
            dst: NEXT_INDEX_REGISTER,
            left: INDEX_REGISTER,
            right: ONE_REGISTER,
            kind: Op::Add,
        },
        DynOp::StoreLocal {
            slot: INDEX_LOCAL,
            src: NEXT_INDEX_REGISTER,
        },
        DynOp::Jump { target: LOOP_START },
    ];
    ops.extend(tail);
    DynCode {
        ops: ops.into_iter().map(instruction).collect(),
        registers: usize::from(NEXT_INDEX_REGISTER) + 1,
        params: Vec::new(),
        hoisted: Vec::new(),
        source_id: None,
        blocks: Vec::new(),
        bindings: Vec::new(),
        is_script: false,
    }
}

fn static_property_loop_code(property_local: usize) -> DynCode {
    const PROPERTY_REGISTER: Register = 1;
    const ADDEND_REGISTER: Register = 2;
    const PROPERTY_RESULT_REGISTER: Register = 3;
    const INDEX_REGISTER: Register = 4;
    const ONE_REGISTER: Register = 5;
    const NEXT_INDEX_REGISTER: Register = 6;
    const PROPERTY_LOOP_EXIT: usize = 10;
    let ops = vec![
        DynOp::LoadLocal {
            dst: OBJECT_REGISTER,
            slot: property_local,
        },
        DynOp::GetStatic {
            dst: PROPERTY_REGISTER,
            object: OBJECT_REGISTER,
            key: "coordinate".to_owned(),
        },
        DynOp::LoadLocal {
            dst: ADDEND_REGISTER,
            slot: BOUND_LOCAL,
        },
        DynOp::Binary {
            dst: PROPERTY_RESULT_REGISTER,
            left: PROPERTY_REGISTER,
            right: ADDEND_REGISTER,
            kind: Op::Add,
        },
        DynOp::SetStatic {
            object: OBJECT_REGISTER,
            key: "coordinate".to_owned(),
            src: PROPERTY_RESULT_REGISTER,
        },
        DynOp::LoadLocal {
            dst: INDEX_REGISTER,
            slot: INDEX_LOCAL,
        },
        DynOp::LoadLiteral {
            dst: ONE_REGISTER,
            value: Literal::Number(ONE),
        },
        DynOp::Binary {
            dst: NEXT_INDEX_REGISTER,
            left: INDEX_REGISTER,
            right: ONE_REGISTER,
            kind: Op::Add,
        },
        DynOp::StoreLocal {
            slot: INDEX_LOCAL,
            src: NEXT_INDEX_REGISTER,
        },
        DynOp::Jump { target: LOOP_START },
        DynOp::Return { src: None },
    ];
    DynCode {
        ops: ops.into_iter().map(instruction).collect(),
        registers: usize::from(NEXT_INDEX_REGISTER) + 1,
        params: Vec::new(),
        hoisted: Vec::new(),
        source_id: None,
        blocks: vec![
            (LOOP_START, PROPERTY_LOOP_EXIT, true),
            (PROPERTY_LOOP_EXIT, PROPERTY_LOOP_EXIT + 1, false),
        ],
        bindings: Vec::new(),
        is_script: false,
    }
}

fn atom(pc: usize) -> Region<NumericDenseState, NumericDenseState> {
    Region::new(RegionNode::Op(RegionOp::NumberLiteral {
        pc,
        dst: ONE_REGISTER,
        bits: ONE.to_bits(),
    }))
}

#[test]
fn free_sequence_makes_category_laws_structural() {
    let left = (atom(0) + atom(1)) + atom(2);
    let right = atom(0) + (atom(1) + atom(2));
    assert_eq!(left, right);
    assert_eq!(identity() + left.clone(), left);
    assert_eq!(right.clone() + identity(), right);
    assert!(matches!(right.node(), RegionNode::Seq(parts) if parts.len() == 3));
}

#[test]
fn quotes_dense_loop_and_derives_guards_labels_and_effects() {
    let code = loop_code(vec![DynOp::Return { src: None }]);
    let analysis = analyze(&code);
    assert!(analysis.rejected.is_empty());
    let region = &analysis.loops[0];
    assert_eq!(region.start, LOOP_START);
    assert_eq!(region.exit, LOOP_EXIT);
    assert_eq!(region.op_count(), region.end - region.start);
    assert_eq!(region.labels(), vec![LOOP_START, LOOP_EXIT]);
    assert!(
        region
            .requirements()
            .contains(&(GuardSource::Local(OBJECT_LOCAL), GuardKind::DenseArray))
    );
    assert!(
        region
            .requirements()
            .contains(&(GuardSource::Local(INDEX_LOCAL), GuardKind::ArrayIndex))
    );
    assert_eq!(region.proven_index_sites(), &[5, 8]);
    assert!(
        region
            .effects()
            .contains(&RegionEffect::WriteDense(OBJECT_REGISTER))
    );
}

#[test]
fn index_proof_rejects_a_loop_carried_subtraction() {
    let mut code = loop_code(vec![DynOp::Return { src: None }]);
    let DynOp::Binary { kind, .. } = &mut code.ops[10].op else {
        panic!("loop update remains a binary operation");
    };
    *kind = Op::Sub;
    let analysis = analyze(&code);
    assert!(analysis.rejected.is_empty());
    assert!(analysis.loops[0].proven_index_sites().is_empty());
    assert!(
        analysis.loops[0]
            .requirements()
            .contains(&(GuardSource::Local(INDEX_LOCAL), GuardKind::Number))
    );
}

#[test]
fn rejects_escaping_temporary() {
    let code = loop_code(vec![DynOp::Return {
        src: Some(RESULT_REGISTER),
    }]);
    assert!(matches!(
        analyze(&code).rejected[0].reason,
        RejectReason::EscapingTemporary(RESULT_REGISTER)
    ));
}

#[test]
fn rejects_exceptional_or_calling_regions() {
    let mut code = loop_code(vec![DynOp::Return { src: None }]);
    code.ops[6].op = DynOp::PushHandler { target: LOOP_EXIT };
    assert!(matches!(
        analyze(&code).rejected[0].reason,
        RejectReason::UnsupportedOpcode {
            opcode: "PushHandler",
            ..
        }
    ));
}

#[test]
fn rejects_external_entries_and_type_alias_conflicts() {
    let mut external = loop_code(vec![DynOp::Jump { target: 5 }]);
    external.ops.push(instruction(DynOp::Return { src: None }));
    assert!(external.ops.iter().enumerate().any(
        |(pc, instruction)| matches!(instruction.op, DynOp::Jump { target: 5 } if pc >= LOOP_EXIT)
    ));
    assert!(analyze(&external).rejected.iter().any(|rejected| matches!(
        rejected.reason,
        RejectReason::ExternalEntry { target: 5, .. }
    )));

    let mut conflict = loop_code(vec![DynOp::Return { src: None }]);
    conflict.ops[1].op = DynOp::Move {
        dst: INDEX_REGISTER,
        src: OBJECT_REGISTER,
    };
    assert!(matches!(
        analyze(&conflict).rejected[0].reason,
        RejectReason::TypeConflict(GuardSource::Local(OBJECT_LOCAL))
            | RejectReason::TypeConflict(GuardSource::LiveIn(OBJECT_REGISTER))
    ));
}

#[test]
fn quotes_static_property_loop_and_derives_numeric_views() {
    let code = static_property_loop_code(OBJECT_LOCAL);
    let analysis = analyze(&code);
    assert!(analysis.rejected.is_empty(), "{:?}", analysis.rejected);
    let region = &analysis.loops[0];
    assert_eq!(region.property_requirements().len(), 2);
    assert!(matches!(
        region.property_requirements()[0].access,
        StaticPropertyAccess::Read
    ));
    assert_eq!(
        region.property_requirements()[0].value_kind,
        PropertyValueKind::Number
    );
    assert!(matches!(
        region.property_requirements()[1].access,
        StaticPropertyAccess::Write
    ));

    let receiver = test_object(Object::ordinary(None));
    receiver
        .borrow_mut()
        .props
        .insert("coordinate", Value::Number(1.0));
    let owner = Value::Object(receiver);
    let plan = GuardPlan::from_loop(region);
    let context = plan
        .validate(|source| match source {
            GuardSource::Local(OBJECT_LOCAL) => Some(owner.clone()),
            GuardSource::Local(INDEX_LOCAL | BOUND_LOCAL) => Some(Value::Number(1.0)),
            _ => None,
        })
        .expect("numeric own property validates once at region entry");
    assert_eq!(context.properties.len(), 1);
    assert_eq!(plan.property_sites().len(), 2);
    assert!(plan.property_sites().iter().all(|site| site.property == 0));
    assert!(plan.is_property_register(OBJECT_REGISTER));
}

#[test]
fn quotes_copyable_object_property_without_claiming_it_is_numeric() {
    const BLOCK_END: usize = 8;
    const OWNER_LOCAL: usize = 0;
    const PAYLOAD_LOCAL: usize = 1;
    const NUMBER_LOCAL: usize = 2;
    const OWNER_REGISTER: Register = 0;
    const PAYLOAD_REGISTER: Register = 1;
    const NUMBER_REGISTER: Register = 2;
    const ONE_REGISTER: Register = 3;
    const SUM_REGISTER: Register = 4;
    const PROPERTY_NAME: &str = "payload";
    let ops = vec![
        DynOp::LoadLocal {
            dst: OWNER_REGISTER,
            slot: OWNER_LOCAL,
        },
        DynOp::GetStatic {
            dst: PAYLOAD_REGISTER,
            object: OWNER_REGISTER,
            key: PROPERTY_NAME.to_owned(),
        },
        DynOp::StoreLocal {
            slot: PAYLOAD_LOCAL,
            src: PAYLOAD_REGISTER,
        },
        DynOp::LoadLocal {
            dst: NUMBER_REGISTER,
            slot: NUMBER_LOCAL,
        },
        DynOp::LoadLiteral {
            dst: ONE_REGISTER,
            value: Literal::Number(ONE),
        },
        DynOp::Binary {
            dst: SUM_REGISTER,
            left: NUMBER_REGISTER,
            right: ONE_REGISTER,
            kind: Op::Add,
        },
        DynOp::StoreLocal {
            slot: NUMBER_LOCAL,
            src: SUM_REGISTER,
        },
        DynOp::Jump { target: BLOCK_END },
        DynOp::Return { src: None },
    ];
    let code = DynCode {
        ops: ops.into_iter().map(instruction).collect(),
        registers: usize::from(SUM_REGISTER) + 1,
        params: Vec::new(),
        hoisted: Vec::new(),
        source_id: None,
        blocks: vec![(0, BLOCK_END, false), (BLOCK_END, BLOCK_END + 1, false)],
        bindings: Vec::new(),
        is_script: false,
    };

    let region = quote_block(&code, 0, BLOCK_END).expect("quote mixed copy/numeric block");
    assert_eq!(region.property_requirements().len(), 1);
    assert_eq!(
        region.property_requirements()[0].value_kind,
        PropertyValueKind::TriviallyCopyable
    );
    let payload = Value::Object(test_object(Object::array(None, vec![Value::Number(ONE)])));
    let owner = test_object(Object::ordinary(None));
    owner.borrow_mut().props.insert(PROPERTY_NAME, payload);
    assert!(
        GuardPlan::from_region(&region)
            .validate(|source| match source {
                GuardSource::Local(OWNER_LOCAL) => Some(Value::Object(owner)),
                GuardSource::Local(NUMBER_LOCAL) => Some(Value::Number(ONE)),
                _ => None,
            })
            .is_ok()
    );
}

#[test]
fn composes_property_prologue_with_dense_loop_as_one_typed_region() {
    const PREFIX_END: usize = 8;
    const LOOP_END: usize = 20;
    const OWNER_SLOT: usize = 0;
    const ARRAY_SLOT: usize = 1;
    const LOOP_INDEX_SLOT: usize = 2;
    const LOOP_BOUND_SLOT: usize = 3;
    const PROPERTY_KEY: &str = "values";
    let ops = vec![
        DynOp::LoadLocal {
            dst: 0,
            slot: OWNER_SLOT,
        },
        DynOp::GetStatic {
            dst: 1,
            object: 0,
            key: PROPERTY_KEY.into(),
        },
        DynOp::StoreLocal {
            slot: ARRAY_SLOT,
            src: 1,
        },
        DynOp::LoadLiteral {
            dst: 2,
            value: Literal::Number(0.0),
        },
        DynOp::StoreLocal {
            slot: LOOP_INDEX_SLOT,
            src: 2,
        },
        DynOp::LoadLiteral {
            dst: 3,
            value: Literal::Number(2.0),
        },
        DynOp::Binary {
            dst: 4,
            left: 2,
            right: 3,
            kind: Op::Add,
        },
        DynOp::StoreLocal {
            slot: LOOP_BOUND_SLOT,
            src: 4,
        },
        DynOp::LoadLocal {
            dst: 5,
            slot: ARRAY_SLOT,
        },
        DynOp::LoadLocal {
            dst: 6,
            slot: LOOP_INDEX_SLOT,
        },
        DynOp::LoadLocal {
            dst: 7,
            slot: LOOP_BOUND_SLOT,
        },
        DynOp::Binary {
            dst: 8,
            left: 6,
            right: 7,
            kind: Op::Lt,
        },
        DynOp::JumpIfFalse {
            test: 8,
            target: LOOP_END,
        },
        DynOp::GetComputed {
            dst: 9,
            object: 5,
            key: 6,
        },
        DynOp::LoadLiteral {
            dst: 10,
            value: Literal::Number(ONE),
        },
        DynOp::Binary {
            dst: 11,
            left: 9,
            right: 10,
            kind: Op::Add,
        },
        DynOp::SetComputed {
            object: 5,
            key: 6,
            src: 11,
            accessor: None,
        },
        DynOp::Binary {
            dst: 12,
            left: 6,
            right: 10,
            kind: Op::Add,
        },
        DynOp::StoreLocal {
            slot: LOOP_INDEX_SLOT,
            src: 12,
        },
        DynOp::Jump { target: PREFIX_END },
        DynOp::Return { src: None },
    ];
    let code = DynCode {
        ops: ops.into_iter().map(instruction).collect(),
        registers: 13,
        params: Vec::new(),
        hoisted: Vec::new(),
        source_id: None,
        blocks: vec![(0, PREFIX_END, false), (PREFIX_END, LOOP_END, true)],
        bindings: Vec::new(),
        is_script: false,
    };

    let region = quote_adjacent_loop(&code, 0, PREFIX_END, LOOP_END)
        .expect("prefix and trace compose through their shared context");
    assert!(matches!(region.region.node(), RegionNode::Seq(parts) if parts.len() == 2));
    assert!(!region.requirements().iter().any(|requirement| {
        requirement == &(GuardSource::Local(ARRAY_SLOT), GuardKind::DenseArray)
    }));
    assert_eq!(
        region.property_requirements()[0].value_kind,
        PropertyValueKind::DenseArray
    );
    let array = Value::Object(test_object(Object::array(
        None,
        vec![Value::Number(1.0), Value::Number(2.0)],
    )));
    let owner = test_object(Object::ordinary(None));
    owner.borrow_mut().props.insert(PROPERTY_KEY, array);
    let plan = GuardPlan::from_region(&region);
    let context = plan
        .validate(|source| match source {
            GuardSource::Local(OWNER_SLOT) => Some(Value::Object(owner)),
            GuardSource::Local(LOOP_INDEX_SLOT | LOOP_BOUND_SLOT) => Some(Value::Number(0.0)),
            _ => None,
        })
        .expect("dense property is validated and installed at the composed entry");
    assert_eq!(plan.dense_view_count(), 1);
    assert_eq!(context.arrays.len(), 1);
    assert_eq!(context.properties.len(), 1);
    assert_eq!(plan.dense_sites().len(), 2);
    assert!(plan.dense_sites().iter().all(|site| site.array == 0));
}

#[test]
fn quotes_straight_numeric_block_as_a_non_trace_region() {
    const BLOCK_END: usize = 8;
    const FIRST_LOCAL: usize = 0;
    const SECOND_LOCAL: usize = 1;
    const FIRST_VALUE: Register = 0;
    const LITERAL_VALUE: Register = 1;
    const SUM_VALUE: Register = 2;
    const SECOND_VALUE: Register = 3;
    const PRODUCT_VALUE: Register = 4;
    let ops = vec![
        DynOp::LoadLocal {
            dst: FIRST_VALUE,
            slot: FIRST_LOCAL,
        },
        DynOp::LoadLiteral {
            dst: LITERAL_VALUE,
            value: Literal::Number(ONE),
        },
        DynOp::Binary {
            dst: SUM_VALUE,
            left: FIRST_VALUE,
            right: LITERAL_VALUE,
            kind: Op::Add,
        },
        DynOp::StoreLocal {
            slot: FIRST_LOCAL,
            src: SUM_VALUE,
        },
        DynOp::LoadLocal {
            dst: SECOND_VALUE,
            slot: SECOND_LOCAL,
        },
        DynOp::Binary {
            dst: PRODUCT_VALUE,
            left: SUM_VALUE,
            right: SECOND_VALUE,
            kind: Op::Mul,
        },
        DynOp::StoreLocal {
            slot: SECOND_LOCAL,
            src: PRODUCT_VALUE,
        },
        DynOp::Jump { target: BLOCK_END },
        DynOp::Return { src: None },
    ];
    let code = DynCode {
        ops: ops.into_iter().map(instruction).collect(),
        registers: usize::from(PRODUCT_VALUE) + 1,
        params: Vec::new(),
        hoisted: Vec::new(),
        source_id: None,
        blocks: vec![(0, BLOCK_END, false), (BLOCK_END, BLOCK_END + 1, false)],
        bindings: Vec::new(),
        is_script: false,
    };

    let region = quote_block(&code, 0, BLOCK_END).expect("quote numeric block");
    assert_eq!(region.start, 0);
    assert_eq!(region.end, BLOCK_END);
    assert!(matches!(region.region.node(), RegionNode::Seq(_)));
    assert!(
        region
            .requirements()
            .contains(&(GuardSource::Local(FIRST_LOCAL), GuardKind::Number))
    );
    assert!(
        region
            .requirements()
            .contains(&(GuardSource::Local(SECOND_LOCAL), GuardKind::Number))
    );
}

#[test]
fn rejects_property_receiver_local_overwritten_inside_region() {
    let code = static_property_loop_code(INDEX_LOCAL);
    assert!(matches!(
        analyze(&code).rejected[0].reason,
        RejectReason::TypeConflict(GuardSource::Local(INDEX_LOCAL))
    ));
}
