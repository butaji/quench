//! A whole lane function compiled for the target "ahead" (`LaneHelpers::Ahead`): every helper
//! a symbol, linked by relocation into a process that did not compile it.
use super::super::lane_region::{relocs, Completion, Coverage, ElementViews, NO_ENTRY_FIELDS, lower};
use super::*;
use crate::vm::jit_backend::relocatable_code::RelocatableCode;
use std::collections::BTreeMap;

/// `source`'s function 1 lowered ahead and compiled to an image, with its caller registers.
fn ahead_image(source: &str) -> (RelocatableCode, usize) {
    let program = crate::Engine::specialize(source, "ahead-image.js").unwrap();
    let unit = lane_unit::unit(&program, 1, program.function(1), &|_, _| Vec::new(), &|_| None).unwrap();
    let isa = crate::vm::jit_backend::target_isa().unwrap();
    let region = lower(&program, 1, &unit, LaneHelpers::Ahead, &BTreeMap::new(), &Default::default(),
        VmFacts::STATIC, &[], &BTreeMap::new(), &[], &*isa, &NO_ENTRY_FIELDS, &[], &[], &[], &[],
        Coverage::Full, Completion::StatusAndPc, ElementViews::Dense).unwrap();
    let image = crate::vm::jit_backend::compile_function(region.function, &*isa, None, |compiled, function| {
        RelocatableCode::extract(compiled, function).ok_or(NativeCompileRejection::Relocations)
    }).unwrap();
    (image, usize::from(unit.caller_registers))
}

#[test]
fn a_lane_function_compiled_ahead_names_only_helper_symbols_and_runs_once_linked() {
    let (image, registers) = ahead_image("function pick(o){return o?1:2;}");
    assert!(!image.relocations.is_empty(), "the truthiness helper is a symbol");
    assert!(image.relocations.iter().all(|relocation| relocs::helper_key(relocation.namespace, relocation.key).is_some()),
        "no address but a helper's is named: {:?}", image.relocations);
    let table = LaneHelper::addresses::<crate::test_host::TestHost>();
    let linked = image.link(0, |namespace, key| table.get(relocs::helper_key(namespace, key)?).map(|address| *address as usize as u64)).unwrap();
    let memory = CodeMemory::new(&linked).unwrap();
    // SAFETY: the image is a lane entry of the shared ABI, its helpers now this process's.
    let entry = unsafe { std::mem::transmute::<*mut u8, super::super::Entry>(memory.pointer()) };
    let mut frame = super::super::LaneFrame::new();
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    frame.dispatch.refresh_object_bases(&vm);
    frame.dispatch.bind_native_storage(&mut vm);
    frame.dispatch.receiver = Value::UNDEFINED.to_bits();
    frame.context.adapter_payload = (&mut frame.dispatch as *mut JsStepDispatch).cast();
    for (input, expected) in [(0.0, 2.0), (5.0, 1.0), (-1.5, 1.0)] {
        let mut words = vec![Value::UNDEFINED; registers];
        words[0] = Value::number(input);
        let mut word = 0;
        let status = unsafe { entry(&mut frame.context, 0, 0, words.as_mut_ptr().cast(), &mut word) };
        assert_eq!(status, super::super::LANE_BODY_RETURNED, "{input}");
        assert_eq!(Value::from_bits(word), Value::number(expected), "{input}");
    }
}
