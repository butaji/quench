use super::*;
use crate::bytecode::{MethodSite, ReceiverPath, SET_THIS_REGISTER};
use crate::staged::test_words::FACTS;

fn call_instruction(op: Op, constructs: bool) -> WideInstruction {
    if constructs {
        WideInstruction::new(op, 4, 5, 6, ImmediateLayout::construct_immediate(2, false, false))
    } else {
        WideInstruction::new(op, 4, 5, 6, ImmediateLayout::call_immediate(7, 2, false, false))
    }
}

fn instruction(op: Op, result: Register, imm: u32) -> WideInstruction {
    WideInstruction::new(op, result, 5, 6, imm)
}

const SITES: [MethodSite; 2] = [
    MethodSite { atom: 8, cache: 3, argument_start: 0, argument_count: 2, receiver_path: ReceiverPath::NONE },
    MethodSite { atom: 9, cache: 4, argument_start: 2, argument_count: 1, receiver_path: ReceiverPath::at(11, 6) },
];
const RECORDS: [Register; 3] = [4, 2, 7];

fn methods() -> DecodeContext<'static> {
    DecodeContext { method_sites: &SITES, ..DecodeContext::EMPTY }
}

#[test]
fn the_operands_of_a_call_and_a_construction_decode_once() {
    let call = Invocation::of(call_instruction(Op::Call, false), DecodeContext::EMPTY).expect("a call decodes");
    assert_eq!(call, Invocation { callee: Callee::Register(5), receiver: Receiver::Register(6),
        arguments: Arguments::Window { first: 7, count: 2 }, result: 4, returns: false, forwards: false });
    let construct = Invocation::of(call_instruction(Op::Construct, true), DecodeContext::EMPTY).expect("a construction decodes");
    assert_eq!((construct.callee, construct.receiver, construct.arguments.count(), construct.result),
        (Callee::Register(5), Receiver::Allocated, 2, 4));
    assert_eq!(Invocation::of(call_instruction(Op::Move, false), DecodeContext::EMPTY), None);
    let decoded = crate::staged::Operation::<u64, ()>::decode(call_instruction(Op::Call, false), DecodeContext::EMPTY);
    assert!(matches!(decoded, Some(crate::staged::Operation::Call(crate::staged::CallOperation::Site(site))) if site == call));
}

#[test]
fn the_forms_with_no_row_have_no_invocation() {
    let eval = ImmediateLayout::call_immediate(7, 2, true, false);
    let parameter_eval = ImmediateLayout::call_immediate(7, 2, false, true);
    let sup = ImmediateLayout::construct_immediate(2, true, false);
    let array = ImmediateLayout::construct_immediate(2, false, true);
    for (op, imm) in [(Op::Call, eval), (Op::Call, parameter_eval), (Op::Construct, sup), (Op::Construct, array)] {
        assert_eq!(Invocation::of(instruction(op, 4, imm), DecodeContext::EMPTY), None, "{op:?}");
    }
    let flagged = instruction(Op::Call, 4 | SET_THIS_REGISTER, ImmediateLayout::call_immediate(7, 2, false, false));
    assert_eq!(Invocation::of(flagged, DecodeContext::EMPTY), None);
}

#[test]
fn a_forwarding_call_decodes_with_its_flag_and_a_returning_one_with_its_own() {
    let imm = ImmediateLayout::call_immediate(7, 2, false, false);
    let forwarding = Invocation::of(instruction(Op::CallForwardingArguments, 4, imm), DecodeContext::EMPTY).expect("a forwarding call decodes");
    assert!(forwarding.forwards && !forwarding.constructs());
    let returning = Invocation::of(instruction(Op::Call, 4 | RETURN_REGISTER, imm), DecodeContext::EMPTY).expect("a returning call decodes");
    assert!(returning.returns && !returning.forwards && returning.result == 4);
}

#[test]
fn a_known_call_names_its_function_and_has_no_receiver_register() {
    let imm = ImmediateLayout::call_immediate(7, 2, false, false);
    let known = Invocation::of(instruction(Op::CallKnown, 4, imm), DecodeContext::EMPTY).expect("a known call decodes");
    assert_eq!((known.callee, known.receiver, known.arguments), (Callee::Known(5), Receiver::Undefined, Arguments::Window { first: 7, count: 2 }));
    assert!(known.admits(CallFacts { forms: true, ..FACTS }), "a tier that reads the form admits it");
    assert!(!known.admits(FACTS), "the register call of a tier that reads no other form");
}

#[test]
fn a_method_call_takes_its_receiver_atom_and_registers_from_the_method_site() {
    let method = Invocation::of(instruction(Op::CallMethod, 4, 0), methods()).expect("a method call decodes");
    assert_eq!((method.callee, method.receiver), (Callee::Method { atom: 8, site: 0 }, Receiver::Register(5)));
    let Arguments::List(list) = method.arguments else { panic!("a method call passes a list") };
    assert_eq!(list.registers(&RECORDS), [4, 2], "the registers need not be consecutive");
    let this = Invocation::of(instruction(Op::CallThisMethod, 4, 1), methods()).expect("a this-method call decodes");
    assert_eq!((this.callee, this.receiver), (Callee::Method { atom: 9, site: 1 }, Receiver::This { path: Some((11, 6)) }));
    assert_eq!(Invocation::of(instruction(Op::CallMethod, 4, 2), methods()), None, "a site the table lacks");
    assert_eq!(Invocation::of(instruction(Op::CallMethod, 4, 0), DecodeContext::EMPTY), None, "a tier with no table");
    assert_eq!(RegisterList { start: 2, count: 5 }.registers(&RECORDS), [] as [Register; 0], "records past the table name none");
}

#[test]
fn a_tier_admits_the_calls_whose_window_and_registers_it_holds() {
    let call = Invocation { callee: Callee::Register(1), receiver: Receiver::Register(2),
        arguments: Arguments::Window { first: 7, count: 3 }, result: 0, returns: false, forwards: false };
    assert!(call.admits(FACTS), "a window ending at the last register fits");
    assert!(!Invocation { arguments: Arguments::Window { first: 7, count: 4 }, ..call }.admits(FACTS), "a window past the registers");
    assert!(!Invocation { callee: Callee::Register(10), ..call }.admits(FACTS), "a callee the tier does not hold");
    assert!(!call.admits(CallFacts { activates: false, ..FACTS }), "a tier that completes no activation");
    let forwarding = Invocation { forwards: true, ..call };
    assert!(!forwarding.admits(FACTS) && forwarding.admits(CallFacts { forwarding: true, ..FACTS }));
}
