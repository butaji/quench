//! Lane activation storage and general-pop accounting, not a whole call/return cost.
//! Script fast returns bypass `pop_activation`; guest compositions cover that path.

use super::*;

const ACTIVATIONS: usize = 64;
const WASM_REGISTERS: usize = 7;
const WASM_STORAGE_BYTES: usize = WASM_REGISTERS * size_of::<Value>();

#[derive(Clone, Copy)]
enum Kinds { Script, Wasm, Mixed }

fn activation(kinds: Kinds, index: usize) -> Activation {
    let wasm = match kinds {
        Kinds::Script => false,
        Kinds::Wasm => true,
        Kinds::Mixed => !index.is_multiple_of(2),
    };
    Activation {
        function: index as u32,
        call: std::ptr::null(),
        caller_base: std::ptr::null(),
        caller_registers: Registers(std::ptr::null_mut()),
        window: index * WASM_REGISTERS,
        caller_program: ProgramId::MAIN,
        kind: if wasm {
            ActivationKind::Wasm { memory: MemoryView::NONE, frame_storage_bytes: WASM_STORAGE_BYTES }
        } else {
            ActivationKind::Script(ScriptActivation {
                callee: Value::UNDEFINED,
                argc: 0,
                caller_this: Value::NULL,
                caller_env: Value::UNDEFINED,
                caller_capture: std::ptr::null(),
                completion: Completion::Plain,
                forwards: false,
                environment: false,
            })
        },
    }
}

#[test]
fn activation_storage_tracks_mixed_push_pop_and_empty_stack() {
    println!("Activation={} align={} ActivationKind={} ScriptActivation={} MemoryView={} reserved_batch_bytes={}",
        size_of::<Activation>(), align_of::<Activation>(), size_of::<ActivationKind>(),
        size_of::<ScriptActivation>(), size_of::<MemoryView>(), ACTIVATIONS * size_of::<Activation>());
    for kinds in [Kinds::Script, Kinds::Wasm, Kinds::Mixed] {
        let mut stack = LaneStack::default();
        stack.activations.reserve(ACTIVATIONS);
        let mut expected = 0;
        for index in 0..ACTIVATIONS {
            let entry = activation(kinds, index);
            let bytes = if entry.script().is_some() { 0 } else { WASM_STORAGE_BYTES };
            expected += bytes;
            // SAFETY: capacity was reserved for the complete batch.
            unsafe { stack.push_activation(entry) };
            assert_eq!(stack.wasm_storage_bytes, expected);
        }
        for index in (0..ACTIVATIONS).rev() {
            let entry = stack.pop_activation().unwrap();
            assert_eq!(entry.function, index as u32);
            assert_eq!(entry.window, index * WASM_REGISTERS);
            assert_eq!(entry.caller_program, ProgramId::MAIN);
            let bytes = if entry.script().is_some() { 0 } else { WASM_STORAGE_BYTES };
            expected -= bytes;
            assert_eq!(stack.wasm_storage_bytes, expected);
        }
        assert!(stack.pop_activation().is_none());
        assert_eq!(stack.wasm_storage_bytes, 0);
    }
}

#[cfg(feature = "bench-primitives")]
mod measurements {
    extern crate test;
    use super::*;
    use std::hint::black_box;
    use test::Bencher;

    fn batch(b: &mut Bencher, kinds: Kinds) {
        let mut stack = LaneStack::default();
        stack.activations.reserve(ACTIVATIONS);
        b.iter(|| {
            let stack = black_box(&mut stack);
            for index in 0..ACTIVATIONS {
                // SAFETY: each iteration starts empty and capacity holds this batch.
                unsafe { stack.push_activation(activation(kinds, index)) };
            }
            black_box(stack.activations.as_slice());
            black_box(stack.wasm_storage_bytes);
            for _ in 0..ACTIVATIONS {
                black_box(stack.pop_activation().unwrap());
            }
        });
        assert!(stack.activations.is_empty());
        assert_eq!(stack.wasm_storage_bytes, 0);
    }

    #[bench]
    fn primitive_activation_script_storage_batch(b: &mut Bencher) { batch(b, Kinds::Script); }
    #[bench]
    fn primitive_activation_wasm_storage_batch(b: &mut Bencher) { batch(b, Kinds::Wasm); }
    #[bench]
    fn primitive_activation_mixed_storage_batch(b: &mut Bencher) { batch(b, Kinds::Mixed); }
}
