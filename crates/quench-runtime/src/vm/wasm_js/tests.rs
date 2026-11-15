use std::{cell::RefCell, rc::Rc};
use crate::{Engine, ExecutionMode, Runtime, test_host::TestHost};

const EMPTY_MODULE: &[u8] = b"\0asm\x01\0\0\0";

#[test]
fn shared_memory_views_preserve_aliasing_and_lengths_across_growth() {
    let source = r#"
        const memory = new WebAssembly.Memory({initial: 1, maximum: 3, shared: true});
        const before = memory.buffer;
        const integers = new Int32Array(before);
        Atomics.store(integers, 0, 42);
        if (memory.grow(1) !== 1) throw 'wrong previous size';
        const after = memory.buffer;
        if (!(before instanceof SharedArrayBuffer) || !(after instanceof SharedArrayBuffer)
            || before === after || memory.buffer !== after
            || before.byteLength !== 65536 || after.byteLength !== 131072) throw 'wrong shared views';
        Atomics.add(new Int32Array(after), 0, 1);
        if (integers[0] !== 43 || new DataView(before).getInt32(0, true) !== 43) throw 'lost alias';
        new Uint8Array(after)[65536] = 17;
        new Uint8Array(before, 8, 1).set(new Uint8Array(after, 65536, 1));
        if (new Uint8Array(before)[8] !== 17) throw 'cross-prefix copy failed';
        memory.grow(0);
        if (memory.buffer === after || after.byteLength !== 131072) throw 'wrong grow-zero identity';
        const ordinary = new WebAssembly.Memory({initial: 1});
        const old = ordinary.buffer;
        ordinary.grow(0);
        if (old.byteLength !== 0 || old === ordinary.buffer) throw 'ordinary buffer not detached';
    "#;
    let program = Engine::specialize(source, "shared-memory-views.js").unwrap();
    for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
        let mut runtime = Runtime::new(TestHost::default());
        runtime.set_execution_mode(mode);
        runtime.execute(&program).unwrap();
    }
}

#[test]
fn validation_runs_without_a_host_compiler_in_interpreter_and_jit_modes() {
    fn validate(host: &mut TestHost, bytes: &[u8], mode: ExecutionMode) -> bool {
        assert_eq!(bytes, EMPTY_MODULE);
        host.state::<RefCell<Vec<ExecutionMode>>>().borrow_mut().push(mode);
        true
    }
    let source = format!("if (!WebAssembly.validate(new Uint8Array({EMPTY_MODULE:?}))) throw 'validation required a compiler';");
    let program = Engine::specialize(&source, "validation-only.js").unwrap();
    for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
        let calls = Rc::new(RefCell::new(Vec::<ExecutionMode>::new()));
        let host = TestHost::default().with_state(calls.clone()).with_wasm_validation(validate);
        let mut runtime = Runtime::new(host);
        runtime.set_execution_mode(mode);
        runtime.execute(&program).unwrap();
        assert_eq!(*calls.borrow(), [mode]);
    }
}
