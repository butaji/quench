use crate::{Engine, Host, NativeContext, RootId, RootedError, Runtime};
use std::cell::RefCell;
use std::rc::Rc;

#[cfg(feature = "aot")]
#[test]
fn native_loop_calls_return_their_integer_state_with_a_captured_constant() {
    let body = r#"
        const feedback = 0xD008;
        function state(n, seed) {
            n |= 0; let x = seed | 0;
            for (let i = 0; i < n; i = (i + 1) | 0) {
                if ((x & 1) === 0) x = x >> 1;
                else x = (x >> 1) ^ feedback;
            }
            return x | 0;
        }
        for (let warm = 0; warm < 64; warm++) state(1, warm);
        const args = ['-1', '0', '123456789', '1', '17'];
        print(state(Number(args[0]), Number(args[1])));
        print(state(Number(args[1]), Number(args[2])));
        print(state(Number(args[3]), Number(args[2])));
        print(state(Number(args[4]), Number(args[2])));
    "#;
    let method = format!("const sink = {{ log: print }}; {}", body.replace("print(state(", "sink.log(state("));
    for source in [body.to_owned(), format!("(function () {{ {body} }})();"), format!("(function () {{ {method} }})();")] {
        let program = Engine::specialize(&source, "native-loop-result.js").unwrap();
        let mut expected = Runtime::new(crate::test_host::TestHost::default());
        expected.set_execution_mode(crate::ExecutionMode::Interpreted);
        expected.execute(&program).unwrap();
        let expected = expected.host_mut().lines().to_vec();
        let native = crate::NativeJsProgram::compile_eager(program.clone()).unwrap();
        let mut builder = Runtime::new(crate::test_host::TestHost::default());
        let bytes = builder.build_snapshot(&program, |_, _| Ok(())).unwrap();
        let mut runtime = Runtime::new(crate::test_host::TestHost::default());
        runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
        runtime.execute_native_from_snapshot(Rc::new(bytes), &native, |_, _| Ok(())).unwrap();
        assert_eq!(runtime.host_mut().lines().to_vec(), expected, "{source}");
    }
}

#[cfg(feature = "aot")]
#[test]
fn native_bootstrap_preserves_static_namespace_and_retains_code_for_jobs() {
    let program = Engine::specialize(r#"
        function fold(n, x) { for (let i = 0; i < n; i++) x = (x ^ (x << 3)) | 0; return x; }
        print(fold(17, 7));
        Promise.resolve(9).then(value => print(value));
    "#, "native-bootstrap.js").unwrap();
    let mut builder = Runtime::new(crate::test_host::TestHost::default());
    let bytes = builder.build_snapshot(&program, |_, _| Ok(())).unwrap();
    let mut expected = Runtime::new(crate::test_host::TestHost::default());
    expected.execute(&program).unwrap();
    let expected = expected.host_mut().lines().to_vec();
    for mode in [crate::ExecutionMode::Compiled, crate::ExecutionMode::NativeOnly] {
        let native = crate::NativeJsProgram::compile_eager(program.clone()).unwrap();
        let mut runtime = Runtime::new(crate::test_host::TestHost::default());
        runtime.set_execution_mode(mode);
        runtime.execute_native_from_snapshot(Rc::new(bytes.clone()), &native, |_, _| Ok(())).unwrap();
        let program = native.residual().clone();
        drop(native);
        runtime.run_host_jobs(&program).unwrap();
        assert_eq!(&*runtime.host_mut().lines(), &expected);
        assert_eq!(runtime.native_execution_rejections(), 0);
    }
}

#[cfg(feature = "aot")]
#[test]
fn native_bootstrap_rejects_namespace_or_specialization_mismatch_before_entry() {
    let program = Engine::specialize("function original() { print('effect'); } original();", "native-bootstrap.js").unwrap();
    let mut builder = Runtime::new(crate::test_host::TestHost::default());
    let bytes = builder.build_snapshot(&program, |_, _| Ok(())).unwrap();
    for (foreign, message) in [
        (Engine::specialize("function foreign() { print('effect'); } foreign();", "foreign.js").unwrap(), "atom IDs"),
        (Engine::specialize("function foreign(argument) { print(extra); } foreign();", "foreign.js").unwrap(), "atom IDs"),
        (Engine::specialize_unspecialized("function original() { print('effect'); } original();", "native-bootstrap.js").unwrap(), "specialization"),
    ] {
        let native = crate::NativeJsProgram::compile_eager(foreign).unwrap();
        let mut runtime = Runtime::new(crate::test_host::TestHost::default());
        runtime.set_execution_mode(crate::ExecutionMode::NativeOnly);
        let error = runtime.execute_native_from_snapshot(Rc::new(bytes.clone()), &native, |_, _| Ok(())).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
        assert!(runtime.host_mut().lines().is_empty());
    }
}

/// Global code a host runs while installing: what a snapshot keeps.
const SETUP: &str = r#"
    var counter = 0;
    function bump() { return ++counter; }
    var pattern = /a(b+)c/g;
    var Point = class Point {
        #x;
        constructor(x) { this.#x = x; }
        get x() { return this.#x; }
        #twice() { return this.#x * 2; }
        twice() { return this.#twice(); }
        static #made = 0;
        static make(x) { Point.#made++; return new Point(x); }
        static has(object) { return #x in object; }
        static get count() { return Point.#made; }
    };
    var kept = new Point(9);
    let lexical = 'lexical';
    function readLexical() { return lexical; }
    var map = new Map([[1, 'one'], [2, 'two']]);
    var set = new Set(['member']);
    var registered = Symbol.for('registered');
    var local = Symbol('local');
    var record = { [local]: 7, nested: { deep: [1, 2, 3] } };
    Object.defineProperty(record, 'hidden', { value: 3, enumerable: false });
    var bound = bump.bind(null);
    var bytes = new Uint8Array([1, 2, 3]);
    var epoch = new Date(0);
    var weak = new WeakMap([[record, 'weak']]);
    var settled = Promise.resolve('settled');
    var template = (strings => strings)`a${1}b`;
    (function () { globalThis.resumed = counter + 100; })
"#;

/// The entry a restored runtime runs against the setup's realm.
const ENTRY: &str = r#"
    print(bump(), bump(), bound(), resumed);
    print(readLexical(), pattern.exec('xabbc')[1], Point.make(4).x, typeof Point, Point.has(kept), Point.has({}), kept.x, kept.twice(), Point.count);
    print(map.get(2), set.has('member'), Symbol.for('registered') === registered);
    print(record[local], JSON.stringify(record), Object.getOwnPropertyNames(record).join());
    print(bytes.join(), epoch.toISOString(), weak.get(record), template.raw.join('|'));
    print([3, 1, 2].sort().join(), 'abc'.replace(/b/, 'B'), new Error('failed').message);
    settled.then(value => print(value));
"#;

#[derive(Default)]
struct SnapshotHost {
    output: Rc<RefCell<Vec<String>>>,
    /// Whether `initialize` stops where a snapshot is taken.
    building: bool,
    resume_callback: Option<RootId>,
}

impl SnapshotHost {
    fn finish_installation(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        let callback = context.host_mut().resume_callback.take().expect("installed callback");
        let undefined = context.undefined();
        context.call_rooted(callback, undefined, &[])?;
        context.release_root(callback);
        Ok(())
    }
}

impl Host for SnapshotHost {
    fn write_line(&mut self, line: &str) {
        self.output.borrow_mut().push(line.into());
    }

    fn clock_millis(&mut self) -> f64 {
        0.0
    }

    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        let callback = context.evaluate_script_rooted(SETUP, "setup.js")?;
        let callback = context.retain(callback)?;
        context.host_mut().resume_callback = Some(callback);
        if context.host_mut().building {
            return Ok(());
        }
        Self::finish_installation(context)
    }

    fn resume(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        Self::finish_installation(context)
    }
}

fn output_of(runtime: &mut Runtime<SnapshotHost>, program: &crate::ResidualProgram) -> Vec<String> {
    runtime.execute_restored(program).unwrap();
    runtime.run_host_jobs(program).unwrap();
    runtime.host_mut().output.borrow().clone()
}

#[test]
fn restored_realm_runs_an_entry_as_the_installing_one_does() {
    let entry = Engine::specialize(ENTRY, "entry.js").unwrap();
    let mut direct = Runtime::new(SnapshotHost::default());
    direct.execute_deferred_jobs(&entry).unwrap();
    direct.run_host_jobs(&entry).unwrap();
    let expected = direct.host_mut().output.borrow().clone();
    assert_eq!(expected.len(), 7, "{expected:?}");

    // The builder's own entry differs from the restored one: only its atoms carry over.
    let builder_entry = Engine::specialize("print('builder');", "builder.js").unwrap();
    let mut builder = Runtime::new(SnapshotHost { building: true, ..SnapshotHost::default() });
    let snapshot = builder
        .build_snapshot(&builder_entry, |host, writer| writer.option_root(host.resume_callback))
        .unwrap();
    assert!(builder.host_mut().output.borrow().is_empty());

    for _ in 0..2 {
        let mut restored = Runtime::new(SnapshotHost::default());
        let program = restored
            .restore_snapshot(Rc::new(snapshot.clone()), &entry, |host, reader| {
                host.resume_callback = reader.option_root()?;
                Ok(())
            })
            .unwrap();
        assert_eq!(output_of(&mut restored, &program), expected);
    }
}
