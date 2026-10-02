use crate::vm::Vm;
use crate::{Diagnostic, Engine, Host, JsError, ResidualProgram, RootId, Value};

/// The syntax context used when compiling source.  The v2 compiler currently
/// accepts the Script subset; the other contexts are explicit so callers do
/// not accidentally treat module/eval source as an ordinary script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Script,
    Module,
    Eval,
}

/// A named source unit submitted to the specializer.
#[derive(Clone, Copy, Debug)]
pub struct ExecutionRequest<'a> {
    pub source: &'a str,
    pub name: &'a str,
    pub kind: SourceKind,
}

impl<'a> ExecutionRequest<'a> {
    pub fn script(source: &'a str, name: &'a str) -> Self {
        Self {
            source,
            name,
            kind: SourceKind::Script,
        }
    }
}

impl Engine {
    /// Compile a source unit through the single OXC-to-residual boundary.
    pub fn compile(request: ExecutionRequest<'_>) -> Result<ResidualProgram, Vec<Diagnostic>> {
        match request.kind {
            SourceKind::Script => Self::specialize(request.source, request.name),
            SourceKind::Module => Self::specialize_module(request.source, request.name),
            SourceKind::Eval => Err(vec![Diagnostic::unsupported(
                request.name,
                "eval compilation requires an activation context",
            )]),
        }
    }
}

/// Explicit compile/execute owner.  Keeping the host and VM together prevents
/// callers from bypassing residual validation or creating an uninitialized VM.
pub struct Runtime<H> {
    vm: Vm<H>,
}

impl<H: Host> Runtime<H> {
    pub fn new(host: H) -> Self {
        Self { vm: Vm::new(host) }
    }

    pub fn host_mut(&mut self) -> &mut H {
        &mut self.vm.host
    }

    pub fn into_host(self) -> H {
        self.vm.host
    }

    fn execute_value(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        program.validate().map_err(JsError::validation)?;
        self.vm.execute(program)
    }

    /// Execute without exposing a guest handle to the host.
    pub fn execute(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.execute_value(program).map(drop)
    }

    /// Execute and retain the result under a generation-checked host root.
    pub fn execute_rooted(&mut self, program: &ResidualProgram) -> Result<RootId, JsError> {
        let value = self.execute_value(program)?;
        Ok(self.root(value))
    }

    /// Execute a lowered i32 Wasm function on the same VM as JavaScript.
    /// Like `execute`, this starts a fresh execution and invalidates old roots.
    pub fn execute_wasm(
        &mut self,
        function: &crate::WasmFunction,
        args: &[crate::WasmValue],
    ) -> Result<Option<crate::WasmValue>, JsError> {
        self.vm.execute_wasm(function, args)
    }

    pub fn execute_wasm_i32(
        &mut self,
        function: &crate::WasmI32Function,
        args: &[i32],
    ) -> Result<Option<i32>, JsError> {
        self.vm.execute_wasm_i32(function, args)
    }

    pub fn root(&mut self, value: Value) -> RootId {
        self.vm.root(value)
    }

    pub fn update_root(&mut self, root: RootId, value: Value) -> bool {
        self.vm.update_root(root, value)
    }

    pub fn release_root(&mut self, root: RootId) -> bool {
        self.vm.release_root(root)
    }

    /// Check whether a persistent handle still belongs to this runtime.
    pub fn root_is_live(&self, root: RootId) -> bool {
        self.vm.root_value(root).is_some()
    }

    /// Queue a callback using only generation-checked persistent roots. The
    /// queue owns the values until `run_jobs` drains them through the VM.
    pub fn enqueue_rooted_job(&mut self, callback: RootId, args: &[RootId]) -> bool {
        let Some(callback) = self.vm.root_value(callback) else {
            return false;
        };
        let Some(args) = args
            .iter()
            .copied()
            .map(|root| self.vm.root_value(root))
            .collect::<Option<Vec<_>>>()
        else {
            return false;
        };
        self.vm.enqueue_job(callback, args);
        true
    }

    fn run_jobs_value(&mut self, program: &ResidualProgram) -> Result<Value, JsError> {
        program.validate().map_err(JsError::validation)?;
        self.vm.drain_jobs(program)
    }

    /// Drain queued jobs without exposing a guest handle to the host.
    pub fn run_jobs(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.run_jobs_value(program).map(drop)
    }

    /// Drain queued jobs and retain the completion under a host root.
    pub fn run_jobs_rooted(&mut self, program: &ResidualProgram) -> Result<RootId, JsError> {
        let value = self.run_jobs_value(program)?;
        Ok(self.root(value))
    }

    pub fn format_error(&mut self, program: &ResidualProgram, error: &JsError) -> String {
        self.vm.format_error(program, error)
    }

    /// Force the VM's named GC safepoint. The residual program supplies the
    /// frame root maps used when a host asks for collection between calls.
    pub fn collect(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        program.validate().map_err(JsError::validation)?;
        self.vm.collect_now(program);
        Ok(())
    }

    pub fn compile_and_execute(
        &mut self,
        request: ExecutionRequest<'_>,
    ) -> Result<(), RuntimeError> {
        let program = Engine::compile(request).map_err(RuntimeError::Diagnostics)?;
        self.execute(&program).map_err(RuntimeError::Execution)
    }

    /// Compile, execute, and retain the result under a host-owned root.
    pub fn compile_and_execute_rooted(
        &mut self,
        request: ExecutionRequest<'_>,
    ) -> Result<RootId, RuntimeError> {
        let program = Engine::compile(request).map_err(RuntimeError::Diagnostics)?;
        self.execute_rooted(&program)
            .map_err(RuntimeError::Execution)
    }
}

#[derive(Debug)]
pub enum RuntimeError {
    Diagnostics(Vec<Diagnostic>),
    Execution(JsError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Clone, Default)]
    struct Capture(Rc<RefCell<Vec<String>>>);

    impl Host for Capture {
        fn write_line(&mut self, text: &str) {
            self.0.borrow_mut().push(text.into());
        }

        fn globals(&self) -> &'static [crate::HostGlobal] {
            &[crate::HostGlobal {
                name: "$262",
                capability: crate::CapabilityId::CreateRealm,
            }]
        }

        fn clock_millis(&mut self) -> f64 {
            0.0
        }
    }

    fn assert_output_in_execution_modes(source: &str, expected: &[&str]) {
        for (mode, compile) in [
            ("specialized", Engine::specialize as fn(&str, &str) -> _),
            ("unspecialized", Engine::specialize_unspecialized),
        ] {
            let host = Capture::default();
            let view = host.clone();
            let mut runtime = Runtime::new(host);
            let program = compile(source, "regression.js").unwrap();
            if let Err(error) = runtime.execute(&program) {
                panic!(
                    "{mode}: {} (output: {:?})",
                    runtime.format_error(&program, &error),
                    view.0.borrow().as_slice(),
                );
            }
            assert_eq!(view.0.borrow().as_slice(), expected, "{mode}");
        }
    }

    #[test]
    fn regression_dynamic_eval_bindings_survive_yield_and_await() {
        assert_output_in_execution_modes(
            r#"
            function* syncGenerator() {
                eval('var retained = { value: 42 }');
                yield 0;
                eval('var afterResume = 1');
                $262.gc();
                print(retained.value);
            }
            var iterator = syncGenerator();
            iterator.next();
            $262.gc();
            iterator.next();
            async function asyncFunction() {
                eval('var retained = { value: 43 }');
                await 0;
                eval('var afterResume = 1');
                $262.gc();
                print(retained.value);
            }
            asyncFunction();
            async function* asyncGenerator() {
                eval('var retained = { value: 44 }');
                yield 0;
                await 0;
                eval('var afterResume = 1');
                $262.gc();
                print(retained.value);
            }
            var asyncIterator = asyncGenerator();
            asyncIterator.next().then(function () {
                $262.gc();
                return asyncIterator.next();
            });
            "#,
            &["42", "43", "44"],
        );
    }

    #[test]
    fn regression_eval_preserves_strict_context_for_nested_function_expressions() {
        assert_output_in_execution_modes(
            r#"
            var sink = {};
            var outcomes = [];
            for (var binding of ['eval', 'arguments']) {
                var source = 'sink.fn = function ' + binding + '() {};';
                for (var mode = 0; mode < 3; mode++) {
                    try {
                        if (mode === 0) eval("'use strict'; " + source);
                        else if (mode === 1) {
                            (function () { 'use strict'; eval(source); })();
                        } else (0, eval)("'use strict'; " + source);
                        outcomes.push(false);
                    } catch (error) {
                        outcomes.push(error instanceof SyntaxError);
                    }
                    outcomes.push(Object.hasOwn(sink, 'fn'));
                }
            }
            print(outcomes.join(','));
            eval('sink.fn = function eval() {};');
            print(sink.fn.name);
            print(eval("'use strict'; (function () { return this; })()") === undefined);
            (function () {
                'use strict';
                print(eval('(function () { return this; })()') === undefined);
            })();
            print(eval("'use strict'; sink.eval = 1; sink.eval"));
            (function () {
                'use strict';
                print(eval('sink.arguments = 2; sink.arguments'));
            })();
            "#,
            &[
                "true,false,true,false,true,false,true,false,true,false,true,false",
                "eval",
                "true",
                "true",
                "1",
                "2",
            ],
        );
    }

    #[test]
    fn regression_strict_assignment_checks_distinguish_bindings_and_properties() {
        assert_output_in_execution_modes(
            r#"
            for (var source of [
                'eval = 1;', 'eval++;', '++arguments;',
                '\\u0065val = 1;', '({ value: eval } = { value: 1 });'
            ]) {
                try {
                    eval("'use strict'; " + source);
                    print(false);
                } catch (error) {
                    print(error instanceof SyntaxError);
                }
            }
            print(eval("'use strict'; ({ eval: 1, arguments: 2 }).eval"));
            print(eval("'use strict'; /eval=/.source"));
            "#,
            &["true", "true", "true", "true", "true", "1", "eval="],
        );
    }

    #[test]
    fn regression_derived_constructors_preserve_the_actual_superclass() {
        assert_output_in_execution_modes(
            r#"
            class Base { get answer() { return 42; } }
            class Derived extends Base {}
            print(Object.getPrototypeOf(Derived) === Base);
            print(Object.getPrototypeOf(Derived.prototype) === Base.prototype);
            print(new Derived().answer);
            "#,
            &["true", "true", "42"],
        );
    }

    #[test]
    fn regression_error_results_survive_collecting_message_and_cause() {
        assert_output_in_execution_modes(
            r#"
            for (var Constructor of [Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError]) {
                for (var mode of ['call', 'construct']) {
                    var marker = {rank: 7};
                    var message = {toString() {$262.gc(); return 'message';}};
                    var options = {get cause() {$262.gc(); return marker;}};
                    var error = mode === 'call' ? Constructor(message, options) : new Constructor(message, options);
                    print(error.message);
                    print(error.cause === marker);
                    print(Object.getOwnPropertyDescriptor(error, 'message').enumerable);
                }
            }
            var marker = {rank: 8};
            var suppressed = {rank: 9};
            var error = new SuppressedError(marker, suppressed, {toString() {$262.gc(); return 'suppressed';}});
            print(error.message);
            print(error.error === marker);
            print(error.suppressed === suppressed);
            "#,
            &[
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "message",
                "true",
                "false",
                "suppressed",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_intrinsic_prototype_fallback_ignores_replaced_globals() {
        assert_output_in_execution_modes(
            r#"
            var OriginalObject = Object, OriginalArray = Array, OriginalRegExp = RegExp;
            var objectPrototype = Object.prototype, arrayPrototype = Array.prototype, regexpPrototype = RegExp.prototype;
            var reads = 0;
            for (var name of ['Object', 'Array', 'RegExp']) {
                OriginalObject.defineProperty(globalThis, name, {configurable: true, get() {reads++; $262.gc(); return {prototype: {}};}});
            }
            function Target() {}
            Target.prototype = null;
            print(OriginalObject.getPrototypeOf(new Target()) === objectPrototype);
            print(OriginalObject.getPrototypeOf(Reflect.construct(OriginalArray, [], Target)) === arrayPrototype);
            print(OriginalObject.getPrototypeOf(Reflect.construct(OriginalRegExp, [], Target)) === regexpPrototype);
            print(OriginalObject.getPrototypeOf(new AggregateError([]).errors) === arrayPrototype);
            print(reads);
            "#,
            &["true", "true", "true", "true", "0"],
        );
    }

    #[test]
    fn regression_foreign_intrinsic_prototypes_survive_global_replacement() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm();
            var objectPrototype = foreign.global.Object.prototype;
            var arrayPrototype = foreign.global.Array.prototype;
            var regexpPrototype = foreign.global.RegExp.prototype;
            var functionPrototype = foreign.global.Function.prototype;
            foreign.evalScript('globalThis.Target = function Target() {}; Target.prototype = null;');
            var Target = foreign.global.Target;
            var reads = 0;
            for (var name of ['Object', 'Array', 'RegExp', 'Function']) {
                Object.defineProperty(foreign.global, name, {configurable: true, get() {reads++; $262.gc(); return {prototype: {}};}});
            }
            $262.gc();
            print(Object.getPrototypeOf(new Target()) === objectPrototype);
            print(Object.getPrototypeOf(Reflect.construct(Array, [], Target)) === arrayPrototype);
            print(Object.getPrototypeOf(Reflect.construct(RegExp, [], Target)) === regexpPrototype);
            foreign.evalScript('globalThis.fresh = function fresh() {};');
            print(Object.getPrototypeOf(foreign.global.fresh) === functionPrototype);
            print(Object.getPrototypeOf(foreign.global.fresh.prototype) === objectPrototype);
            print(reads);
            "#,
            &["true", "true", "true", "true", "true", "0"],
        );
    }

    #[test]
    fn regression_dynamic_functions_survive_collecting_prototype_getters() {
        assert_output_in_execution_modes(
            r#"
            var constructors = [Function, (async function() {}).constructor, (function*() {}).constructor, (async function*() {}).constructor];
            for (var Constructor of constructors) {
                var prototype = {rank: 42};
                var reads = 0;
                var target = new Proxy(function Target() {}, {get(object, key, receiver) {
                    if (key === 'prototype') {reads++; $262.gc(); return prototype;}
                    return Reflect.get(object, key, receiver);
                }});
                var result = Reflect.construct(Constructor, ['return 42'], target);
                print(typeof result);
                print(Object.getPrototypeOf(result) === prototype);
                print(Function.prototype.toString.call(result).includes('return 42'));
                print(reads);
            }
            "#,
            &[
                "function", "true", "true", "1", "function", "true", "true", "1", "function", "true",
                "true", "1", "function", "true", "true", "1",
            ],
        );
    }

    #[test]
    fn regression_dynamic_function_fallback_uses_foreign_intrinsics() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm();
            foreign.evalScript('globalThis.kinds = [Function, (async function() {}).constructor, (function*() {}).constructor, (async function*() {}).constructor]; globalThis.Target = function Target() {};');
            var constructors = [Function, (async function() {}).constructor, (function*() {}).constructor, (async function*() {}).constructor];
            var prototypes = foreign.global.kinds.map(function(Constructor) {return Constructor.prototype;});
            var reads = 0;
            for (var name of ['Function', 'AsyncFunction', 'GeneratorFunction', 'AsyncGeneratorFunction']) {
                Object.defineProperty(foreign.global, name, {configurable: true, get() {reads++; $262.gc(); return {prototype: {}};}});
            }
            for (var index = 0; index < constructors.length; index++) {
                var targetReads = 0;
                var target = new Proxy(foreign.global.Target, {get(object, key, receiver) {
                    if (key === 'prototype') {targetReads++; $262.gc(); return null;}
                    return Reflect.get(object, key, receiver);
                }});
                var result = Reflect.construct(constructors[index], ['return 42'], target);
                print(Object.getPrototypeOf(result) === prototypes[index]);
                print(targetReads);
            }
            print(reads);
            for (var Constructor of constructors) {
                var marker = {};
                var target = new Proxy(function Target() {}, {get(object, key, receiver) {
                    if (key === 'prototype') {$262.gc(); throw marker;}
                    return Reflect.get(object, key, receiver);
                }});
                try {Reflect.construct(Constructor, ['return 42'], target); print(false);}
                catch (error) {print(error === marker);}
            }
            "#,
            &[
                "true", "1", "true", "1", "true", "1", "true", "1", "0", "true", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_function_constructor_classes_use_parameters_and_class_bodies() {
        assert_output_in_execution_modes(
            r#"
            class Base {constructor(value) {this.value = value;}}
            var factory = Function('Parent', 'return class Derived extends Parent { constructor(value) {super(value); this.extra = 1;} #private = 8; get twice() {return this.value * 2;} getPrivate() {return this.#private;} static marker() {return 42;} }');
            var Derived = factory(Base);
            var value = new Derived(21);
            print(value.twice);
            print(value.extra);
            print(value.getPrivate());
            print(Derived.marker());
            print(Derived.name);
            print(Object.getPrototypeOf(Derived) === Base);
            print(value instanceof Base);
            var Generator = (function*() {}).constructor;
            var generatorFactory = Generator('Parent', 'return class Derived extends Parent {field = 42;}');
            var Generated = generatorFactory(Base).next().value;
            print(new Generated().field);
            var Async = (async function() {}).constructor;
            Async('Parent', 'return class Derived extends Parent {field = 43;}')(Base).then(function(Generated) {print(new Generated().field);});
            var AsyncGenerator = (async function*() {}).constructor;
            AsyncGenerator('Parent', 'return class Derived extends Parent {field = 44;}')(Base).next().then(function(step) {print(new step.value().field);});
            "#,
            &[
                "42", "1", "8", "42", "Derived", "true", "true", "42", "43", "44",
            ],
        );
    }

    #[test]
    fn regression_dynamic_class_heritage_is_evaluated_when_the_factory_runs() {
        assert_output_in_execution_modes(
            r#"
            var reads = 0;
            var Current = class First {};
            Object.defineProperty(globalThis, 'Parent', {configurable: true, get() {reads++; $262.gc(); return Current;}});
            var factory = Function('return class Derived extends Parent {field = 42; static value = 7;}');
            print(reads);
            Current = class Second {};
            var Derived = factory();
            print(reads);
            print(Object.getPrototypeOf(Derived) === Current);
            print(new Derived().field);
            print(Derived.value);
            print(Derived.name);
            try {Derived(); print(false);} catch (error) {print(error instanceof TypeError);}
            var Missing = Function('return class Derived extends MissingParent {}');
            print(typeof Missing);
            try {Missing(); print(false);} catch (error) {print(error instanceof ReferenceError);}
            try {Function('return class Derived extends Object { missing('); print(false);}
            catch (error) {print(error instanceof SyntaxError);}
            "#,
            &[
                "0", "1", "true", "42", "7", "Derived", "true", "function", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_buffer_intrinsics_ignore_replaced_globals() {
        assert_output_in_execution_modes(
            r#"
            var constructors = [ArrayBuffer, SharedArrayBuffer, DataView];
            var prototypes = constructors.map(function(Constructor) {return Constructor.prototype;});
            var buffer = new ArrayBuffer(8);
            var reads = 0;
            for (var name of ['ArrayBuffer', 'SharedArrayBuffer', 'DataView']) {
                Object.defineProperty(globalThis, name, {configurable: true, get() {reads++; $262.gc(); throw 'global getter';}});
            }
            function Target() {}
            Target.prototype = null;
            for (var index = 0; index < constructors.length; index++) {
                var args = index === 2 ? [buffer] : [8];
                var result = Reflect.construct(constructors[index], args, Target);
                print(Object.getPrototypeOf(result) === prototypes[index]);
                print(result.byteLength);
            }
            print(new constructors[2](buffer).byteLength);
            print(reads);
            "#,
            &["true", "8", "true", "8", "true", "8", "8", "0"],
        );
    }

    #[test]
    fn regression_buffer_fallback_uses_foreign_intrinsics() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm();
            var names = ['ArrayBuffer', 'SharedArrayBuffer', 'DataView'];
            var constructors = [ArrayBuffer, SharedArrayBuffer, DataView];
            var prototypes = names.map(function(name) {return foreign.global[name].prototype;});
            foreign.evalScript('globalThis.Target = function Target() {}; Target.prototype = null;');
            var buffer = new ArrayBuffer(8);
            var reads = 0;
            for (var name of names) {
                Object.defineProperty(foreign.global, name, {configurable: true, get() {reads++; $262.gc(); throw 'foreign getter';}});
            }
            $262.gc();
            for (var index = 0; index < constructors.length; index++) {
                var result = Reflect.construct(constructors[index], index === 2 ? [buffer] : [8], foreign.global.Target);
                print(Object.getPrototypeOf(result) === prototypes[index]);
                print(result.byteLength);
            }
            print(reads);
            "#,
            &["true", "8", "true", "8", "true", "8", "0"],
        );
    }

    #[test]
    fn regression_group_by_retains_values_across_collecting_callbacks() {
        assert_output_in_execution_modes(
            r#"
            for (var group of [Map.groupBy, Object.groupBy]) {
                var nextReads = 0;
                var source = {[Symbol.iterator]() {
                    var index = 0;
                    return {get next() {
                        nextReads++; $262.gc();
                        return function() {
                            $262.gc();
                            if (index === 3) return {done: true};
                            return {get done() {$262.gc(); return false;}, get value() {$262.gc(); return {rank: ++index};}};
                        };
                    }};
                }};
                var result = group(source, function(value, index) {$262.gc(); return value.rank % 2;});
                var odd = group === Map.groupBy ? result.get(1) : result[1];
                var even = group === Map.groupBy ? result.get(0) : result[0];
                print(odd.map(value => value.rank).join(','));
                print(even[0].rank);
                print(nextReads);
            }
            var grouped = Map.groupBy([1, 2], function(value) {$262.gc(); return {rank: value};});
            print(Array.from(grouped.keys()).map(key => key.rank).join(','));
            print(Map.groupBy([1], () => -0).keys().next().value === 0);
            print(Object.is(Map.groupBy([1], () => -0).keys().next().value, -0));
            "#,
            &["1,3", "2", "1", "1,3", "2", "1", "1,2", "true", "false"],
        );
    }

    #[test]
    fn regression_group_by_preserves_iterator_abrupt_completion() {
        assert_output_in_execution_modes(
            r#"
            for (var group of [Map.groupBy, Object.groupBy]) {
                for (var phase of ['next', 'done', 'value', 'callback']) {
                    var closes = 0;
                    var source = {[Symbol.iterator]() {return {
                        next() {
                            $262.gc();
                            if (phase === 'next') throw {kind: phase};
                            return {get done() {$262.gc(); if (phase === 'done') throw {kind: phase}; return false;},
                                get value() {$262.gc(); if (phase === 'value') throw {kind: phase}; return 1;}};
                        },
                        get return() {closes++; $262.gc(); return function() {$262.gc(); throw {kind: 'close'};};}
                    };}};
                    try {group(source, function() {$262.gc(); throw {kind: phase};}); print(false);}
                    catch (error) {print(error.kind);}
                    print(closes);
                }
            }
            var closes = 0;
            var source = {[Symbol.iterator]() {return {next() {return {value: 1, done: false};},
                get return() {closes++; $262.gc(); throw {kind: 'close'};}};}};
            try {Object.groupBy(source, function() {return {[Symbol.toPrimitive]() {$262.gc(); throw {kind: 'key'};}};}); print(false);}
            catch (error) {print(error.kind);}
            print(closes);
            var result = Object.groupBy([{rank: 42}], function() {return {[Symbol.toPrimitive]() {$262.gc(); return '__proto__';}};});
            print(Object.getPrototypeOf(result) === null);
            print(result.__proto__[0].rank);
            var descriptor = Object.getOwnPropertyDescriptor(result, '__proto__');
            print(descriptor.writable && descriptor.enumerable && descriptor.configurable);
            "#,
            &[
                "next", "0", "done", "0", "value", "0", "callback", "1", "next", "0", "done", "0",
                "value", "0", "callback", "1", "key", "1", "true", "42", "true",
            ],
        );
    }

    #[test]
    fn regression_from_entries_retains_collecting_entry_operands() {
        assert_output_in_execution_modes(
            r#"
            var reads = 0;
            var symbol = Symbol('entry');
            var events = [];
            var source = {[Symbol.iterator]() {var index = 0; return {get next() {
                reads++; $262.gc(); return function() {
                    $262.gc(); if (index === 2) return {done:true};
                    var rank = ++index;
                    return {get done() {$262.gc(); return false;}, get value() {
                        $262.gc(); return {get 0() {
                            events.push('key'); $262.gc();
                            return {[Symbol.toPrimitive]() {events.push('convert'); $262.gc(); return rank === 1 ? '__proto__' : symbol;}};
                        }, get 1() {events.push('value'); $262.gc(); return {rank:rank};}};
                    }};
                };
            }};}};
            var result = Object.fromEntries(source);
            print(reads);
            print(events.join(','));
            print(Object.getPrototypeOf(result) === Object.prototype);
            print(result.__proto__.rank);
            print(result[symbol].rank);
            var descriptor = Object.getOwnPropertyDescriptor(result, '__proto__');
            print(descriptor.writable && descriptor.enumerable && descriptor.configurable);
            print(Reflect.ownKeys(result).length);
            var realm = $262.createRealm();
            var foreignObject = realm.global.Object;
            var fromEntries = foreignObject.fromEntries;
            var prototype = foreignObject.prototype;
            Object.defineProperty(realm.global, 'Object', {configurable:true, get() {$262.gc(); throw 'global-read';}});
            var foreign = fromEntries([['answer', 42]]);
            print(Object.getPrototypeOf(foreign) === prototype);
            print(foreign.answer);
            "#,
            &[
                "1",
                "key,value,convert,key,value,convert",
                "true",
                "1",
                "2",
                "true",
                "2",
                "true",
                "42",
            ],
        );
    }

    #[test]
    fn regression_from_entries_avoids_descriptor_prototype_effects() {
        assert_output_in_execution_modes(
            r#"
            var calls = 0;
            var names = ['value', 'writable', 'enumerable', 'configurable', 'get', 'set', 'entry'];
            for (var name of names) {
                var descriptor = Object.create(null);
                descriptor.configurable = true;
                descriptor.set = function() {calls++; $262.gc(); throw {kind:'prototype-hook'};};
                Object.defineProperty(Object.prototype, name, descriptor);
            }
            var result;
            try {result = Object.fromEntries([['entry', {rank:42}], ['entry', {rank:43}]]);}
            finally {for (var name of names) delete Object.prototype[name];}
            print(calls);
            print(result.entry.rank);
            var descriptor = Object.getOwnPropertyDescriptor(result, 'entry');
            print(descriptor.writable && descriptor.enumerable && descriptor.configurable);
            "#,
            &["0", "43", "true"],
        );
    }

    #[test]
    fn regression_from_entries_preserves_iterator_abrupt_completion() {
        assert_output_in_execution_modes(
            r#"
            for (var phase of ['next-getter', 'next', 'done', 'value', 'entry', 'key', 'entry-value', 'convert']) {
                var closes = 0;
                var source = {[Symbol.iterator]() {return {
                    get next() {$262.gc(); if (phase === 'next-getter') throw {kind:phase}; return function() {
                        $262.gc(); if (phase === 'next') throw {kind:phase};
                        return {get done() {$262.gc(); if (phase === 'done') throw {kind:phase}; return false;},
                            get value() {$262.gc(); if (phase === 'value') throw {kind:phase}; if (phase === 'entry') return 1;
                                return {get 0() {$262.gc(); if (phase === 'key') throw {kind:phase};
                                    return {[Symbol.toPrimitive]() {$262.gc(); throw {kind:phase};}};},
                                    get 1() {$262.gc(); if (phase === 'entry-value') throw {kind:phase}; return {rank:42};}};}}
                    ;};},
                    get return() {closes++; $262.gc(); return function() {$262.gc(); throw {kind:'close'};};}
                };}};
                try {Object.fromEntries(source); print(false);}
                catch (error) {print(phase === 'entry' ? error instanceof TypeError : error.kind === phase);}
                print(closes);
            }
            "#,
            &[
                "true", "0", "true", "0", "true", "0", "true", "0", "true", "1", "true", "1", "true",
                "1", "true", "1",
            ],
        );
    }

    #[test]
    fn regression_descriptor_fields_survive_later_collecting_getters() {
        assert_output_in_execution_modes(
            r#"
            for (var define of [Object.defineProperty, Reflect.defineProperty]) {
                var target = {};
                var events = [];
                var descriptor = {enumerable:true, configurable:true,
                    get value() {events.push('value'); $262.gc(); return {rank:42};},
                    get writable() {events.push('writable'); $262.gc(); return true;}};
                define(target, 'answer', descriptor);
                print(target.answer.rank);
                print(events.join(','));
                var descriptor = {enumerable:true, configurable:true,
                    get get() {events.push('get'); $262.gc(); return function() {return 43;};},
                    get set() {events.push('set'); $262.gc(); return function(value) {this.saved = value;};}};
                define(target, 'accessor', descriptor);
                print(target.accessor);
                target.accessor = 44;
                print(target.saved);
                print(events.join(','));
            }
            "#,
            &[
                "42",
                "value,writable",
                "43",
                "44",
                "value,writable,get,set",
                "42",
                "value,writable",
                "43",
                "44",
                "value,writable,get,set",
            ],
        );
    }

    #[test]
    fn regression_descriptor_validation_stops_before_setter_lookup() {
        assert_output_in_execution_modes(
            r#"
            for (var define of [Object.defineProperty, Reflect.defineProperty]) {
                var reads = 0;
                try {define({}, 'answer', {get get() {$262.gc(); return 1;},
                    get set() {reads++; $262.gc(); throw {kind:'setter'};}});}
                catch (error) {print(error instanceof TypeError);}
                print(reads);
            }
            var reads = 0;
            try {Reflect.defineProperty(1, {[Symbol.toPrimitive]() {reads++; throw 'key';}}, {});}
            catch (error) {print(error instanceof TypeError);}
            print(reads);
            "#,
            &["true", "0", "true", "0", "true", "0"],
        );
    }

    #[test]
    fn regression_descriptor_projection_survives_collecting_proxy_lookup() {
        assert_output_in_execution_modes(
            r#"
            var target = {};
            var proxy = new Proxy(target, {get defineProperty() {
                $262.gc(); return function(target, key, descriptor) {
                    $262.gc(); print(Reflect.ownKeys(descriptor).join(','));
                    print(descriptor.value.rank);
                    print(typeof key === 'symbol');
                    Object.defineProperty(target, key, descriptor);
                    return true;
                };
            }});
            print(Reflect.defineProperty(proxy, {[Symbol.toPrimitive]() {$262.gc(); return Symbol('entry');}},
                {enumerable:true, configurable:true, get value() {$262.gc(); return {rank:45};},
                    get writable() {$262.gc(); return true;}}));
            print(target[Object.getOwnPropertySymbols(target)[0]].rank);
            var realm = $262.createRealm();
            var descriptor = realm.global.Object.getOwnPropertyDescriptor({answer:46}, 'answer');
            print(Object.getPrototypeOf(descriptor) === realm.global.Object.prototype);
            print(descriptor.value);
            var revocable = Proxy.revocable({rank:47}, {get defineProperty() {
                revocable.revoke(); $262.gc(); return function(target, key, descriptor) {
                    $262.gc(); print(target.rank); print(descriptor.value.rank); return true;
                };
            }});
            print(Reflect.defineProperty(revocable.proxy, 'entry', {configurable:true,
                get value() {$262.gc(); return {rank:48};}, get writable() {$262.gc(); return true;}}));
            "#,
            &[
                "value,writable,enumerable,configurable",
                "45",
                "true",
                "true",
                "45",
                "true",
                "46",
                "47",
                "48",
                "true",
            ],
        );
    }

    #[test]
    fn regression_definition_modes_execute_after_residual_round_trip() {
        let source = r#"
            var calls = 0, savedDefine = Object.defineProperty;
            Object.defineProperty = function() {calls++; throw 'guest define';};
            var instance, literal;
            try {
                class Encoded {
                    #privateMethod() {return 41;}
                    method() {return this.#privateMethod() + 1;}
                    get value() {return this.stored;}
                    set value(next) {this.stored = next;}
                }
                instance = new Encoded(); instance.value = 43;
                literal = {get value() {return this.stored;}, set value(next) {this.stored = next;}};
                literal.value = 44;
            } finally {Object.defineProperty = savedDefine;}
            print(calls); print(instance.method()); print(instance.value); print(literal.value);
            var prototype = Object.getPrototypeOf(instance);
            var method = Object.getOwnPropertyDescriptor(prototype,'method');
            var accessor = Object.getOwnPropertyDescriptor(prototype,'value');
            var own = Object.getOwnPropertyDescriptor(literal,'value');
            print(method.writable); print(method.enumerable); print(method.configurable);
            print(typeof accessor.get); print(typeof accessor.set); print(accessor.enumerable);
            print(typeof own.get); print(typeof own.set); print(own.enumerable);
        "#;
        for compile in [
            Engine::specialize as fn(&str, &str) -> _,
            Engine::specialize_unspecialized,
        ] {
            let program = compile(source, "definition-modes.js").unwrap();
            let path =
                std::env::temp_dir().join(format!("quench-definition-modes-{}", std::process::id()));
            program.write_binary(&path).unwrap();
            let decoded = ResidualProgram::read_binary(&path);
            std::fs::remove_file(path).unwrap();
            let decoded = decoded.unwrap();
            let host = Capture::default();
            let output = host.clone();
            let mut runtime = Runtime::new(host);
            runtime.execute(&decoded).unwrap();
            assert_eq!(
                output.0.borrow().as_slice(),
                &[
                    "0", "42", "43", "44", "true", "false", "true", "function", "function", "false",
                    "function", "function", "true"
                ]
            );
        }
    }

    #[test]
    fn regression_callable_validation_uses_proxy_target_kind() {
        assert_output_in_execution_modes(
            r#"
            var live = new Proxy(function() {return 42;}, {}), target = {};
            Object.prototype.__defineGetter__.call(target,'live',live); print(target.live);
            var revoked = Proxy.revocable(function() {}, {}); revoked.revoke();
            Object.prototype.__defineGetter__.call(target,'revoked',revoked.proxy);
            print(Object.getOwnPropertyDescriptor(target,'revoked').get === revoked.proxy);
            try {target.revoked; print(false);} catch (error) {print(error instanceof TypeError);}
            print([].map(revoked.proxy).length);
            try {JSON.parse('1',revoked.proxy); print(false);} catch (error) {print(error instanceof TypeError);}
            new Map([['entry',1]]).forEach(new Proxy(function(value) {$262.gc(); print(value);}, {}));
            new Set([2]).forEach(new Proxy(function(value) {$262.gc(); print(value);}, {}));
            print(new WeakMap().getOrInsertComputed({},new Proxy(function() {$262.gc(); return {rank:3};},{})).rank);
            var nested = function() {};
            for (var index = 0; index < 5000; index++) nested = new Proxy(nested, {});
            $262.gc(); Object.prototype.__defineSetter__.call(target,'nested',nested);
            print(Object.getOwnPropertyDescriptor(target,'nested').set === nested);
            "#,
            &["42", "true", "true", "0", "true", "1", "2", "3", "true"],
        );
    }

    #[test]
    fn regression_proxy_set_shares_string_and_symbol_semantics() {
        assert_output_in_execution_modes(
            r#"
            function outcome(action) {
                try {return String(action());} catch (error) {return error instanceof TypeError ? 'TypeError' : 'other error';}
            }
            for (var key of ['entry', Symbol('entry')]) {
                var denied = new Proxy({}, {set() {return false;}});
                print(outcome(function() {return Reflect.set(denied, key, 42);}));
                print(outcome(function() {denied[key] = 42; return 'sloppy';}));
                print(outcome(function() {'use strict'; denied[key] = 42;}));
                print(outcome(function() {return Reflect.set(new Proxy({}, {set: 1}), key, 42);}));
                for (var pair of [[42, 43], [NaN, NaN], [0, -0]]) {
                    var target = {}; Object.defineProperty(target, key, {value:pair[0]});
                    print(outcome(function() {return Reflect.set(new Proxy(target, {set() {return true;}}), key, pair[1]);}));
                }
                var accessor = {}; Object.defineProperty(accessor, key, {get() {return 99;}});
                print(outcome(function() {return Reflect.set(new Proxy(accessor, {set() {return true;}}), key, 42);}));
                var receiver = {rank:43}, events = [];
                var target = {}; Object.defineProperty(target, key, {set(value) {
                    $262.gc(); events.push(this === receiver); events.push(value.rank);
                }});
                print(Reflect.set(new Proxy(target, {set:null}), key, {rank:42}, receiver));
                print(events.join(','));
                var writes = 0, target = {};
                var forwarded = new Proxy(target, {defineProperty(object, observed, descriptor) {
                    $262.gc(); writes++; print(observed === key); print(Object.keys(descriptor).join(','));
                    return Reflect.defineProperty(object, observed, descriptor);
                }});
                print(Reflect.set(forwarded, key, 44)); print(target[key]); print(writes);
                print(Reflect.set(forwarded, key, 45)); print(target[key]); print(writes);
                var revoked = Proxy.revocable({}, {}); revoked.revoke();
                print(outcome(function() {return Reflect.set(revoked.proxy, key, 42);}));
            }
            "#,
            &[
                "false",
                "sloppy",
                "TypeError",
                "TypeError",
                "TypeError",
                "true",
                "TypeError",
                "TypeError",
                "true",
                "true,42",
                "true",
                "value,writable,enumerable,configurable",
                "true",
                "44",
                "1",
                "true",
                "value",
                "true",
                "45",
                "2",
                "TypeError",
                "false",
                "sloppy",
                "TypeError",
                "TypeError",
                "TypeError",
                "true",
                "TypeError",
                "TypeError",
                "true",
                "true,42",
                "true",
                "value,writable,enumerable,configurable",
                "true",
                "44",
                "1",
                "true",
                "value",
                "true",
                "45",
                "2",
                "TypeError",
            ],
        );
    }

    #[test]
    fn regression_static_property_reads_respect_exotic_index_storage() {
        assert_output_in_execution_modes(
            r#"
            function first(value) {return value['0'];}
            print(first([42]));
            print(first(Object.create([43])));
            print(first(new Uint8Array([44])));
            print(first(Object.create(new Uint8Array([45]))));
            print(first(new String('x')));
            print(first(Object.create(new String('y'))));
            var prototype = {'0': 99};
            var array = []; Object.setPrototypeOf(array, prototype);
            print(first(array)); print(first(array));
            array[0] = 46;
            print(first(array));
            function invalid(value) {return value['-0'];}
            var reads = 0;
            Object.defineProperty(Uint8Array.prototype, '-0', {get() {reads++; return 99;}});
            print(invalid(new Uint8Array([47])));
            print(reads);
            "#,
            &[
                "42",
                "43",
                "44",
                "45",
                "x",
                "y",
                "99",
                "99",
                "46",
                "undefined",
                "0",
            ],
        );
    }

    #[test]
    fn regression_promise_keyed_combinators_resolve_each_property_before_reading_the_next() {
        assert_output_in_execution_modes(
            r#"
            function verify(name) {
                var events = [];
                var source = {get first() {events.push('get:first'); return 42;},
                    get later() {events.push('get:later'); return 43;}};
                class Constructor extends Promise {static resolve(value) {
                    events.push('resolve'); delete source.later; $262.gc();
                    return {then(resolve) {events.push('then'); resolve(value);}};
                }}
                Promise[name].call(Constructor, source).then(function(result) {
                    print(events.join(',')); print(Object.keys(result).join(','));
                    print(name === 'allKeyed' ? result.first : result.first.value);
                });
            }
            Object.defineProperty(Array.prototype, Symbol.iterator, {get() {throw 'synthetic iterator';}, configurable:true});
            verify('allKeyed'); verify('allSettledKeyed');
            "#,
            &[
                "get:first,resolve,then",
                "first",
                "42",
                "get:first,resolve,then",
                "first",
                "42",
            ],
        );
    }

    #[test]
    fn regression_promise_combinators_keep_original_error_through_iterator_close() {
        assert_output_in_execution_modes(
            r#"
            class Constructor extends Promise {static resolve() {throw {rank:42};}}
            for (var name of ['all', 'allSettled', 'any', 'race']) {
                var iterable = {[Symbol.iterator]() {return {
                    next() {return {done:false, value:1};},
                    return() {$262.gc(); throw {rank:99};}
                };}};
                (function(name) {Promise[name].call(Constructor, iterable).catch(function(error) {print(name + ':' + error.rank);});})(name);
            }
            "#,
            &["all:42", "allSettled:42", "any:42", "race:42"],
        );
    }

    #[test]
    fn regression_promise_keyed_combinators_root_earlier_values_during_later_getters() {
        assert_output_in_execution_modes(
            r#"
            var symbol = Symbol('entry');
            for (var name of ['allKeyed', 'allSettledKeyed']) {
                var source = {
                    get first() {return {rank:42};},
                    get [symbol]() {$262.gc(); return {rank:43};},
                    get last() {$262.gc(); return {rank:44};}
                };
                (function(name) {Promise[name](source).then(function(result) {
                    print(name === 'allKeyed' ? result.first.rank : result.first.value.rank);
                    print(name === 'allKeyed' ? result[symbol].rank : result[symbol].value.rank);
                    print(name === 'allKeyed' ? result.last.rank : result.last.value.rank);
                });})(name);
            }
            "#,
            &["42", "43", "44", "42", "43", "44"],
        );
    }

    #[test]
    fn regression_promise_combinators_cache_and_root_iterator_next() {
        assert_output_in_execution_modes(
            r#"
            function source() {
                return {[Symbol.iterator]() {
                    var reads = 0, complete = false;
                    return {get next() {
                        print('next:' + ++reads);
                        return function() {
                            $262.gc();
                            Object.defineProperty(this, 'next', {value:function() {throw 'replacement';}});
                            if (complete) return {done:true};
                            complete = true;
                            return {done:false, value:42};
                        };
                    }};
                }};
            }
            for (var name of ['all', 'allSettled', 'any', 'race']) {
                (function(name) {Promise[name](source()).then(function(result) {
                    print(name + ':' + JSON.stringify(result));
                }, function(error) {print('rejected:' + error);});})(name);
            }
            "#,
            &[
                "next:1",
                "next:1",
                "next:1",
                "next:1",
                "all:[42]",
                "allSettled:[{\"status\":\"fulfilled\",\"value\":42}]",
                "any:42",
                "race:42",
            ],
        );
    }

    #[test]
    fn regression_promise_combinators_root_fresh_resolve_methods() {
        assert_output_in_execution_modes(
            r#"
            class Constructor extends Promise {
                static get resolve() {return function(value) {print(this === Constructor); return Promise.resolve(value);};}
            }
            function iterable() {
                return {get [Symbol.iterator]() {$262.gc(); return function() {
                    var complete = false;
                    return {next() {$262.gc(); if (complete) return {done:true}; complete = true; return {done:false, value:42};}};
                };}};
            }
            for (var name of ['all', 'allSettled', 'any', 'race']) {
                (function(name) {Promise[name].call(Constructor, iterable()).then(function(value) {
                    print(name + ':' + JSON.stringify(value));
                }, function(error) {print(name + ':rejected:' + error.name);});})(name);
            }
            "#,
            &[
                "true",
                "true",
                "true",
                "true",
                "all:[42]",
                "allSettled:[{\"status\":\"fulfilled\",\"value\":42}]",
                "any:42",
                "race:42",
            ],
        );
    }

    #[test]
    fn regression_promise_combinators_root_custom_capabilities_before_resolve_lookup() {
        assert_output_in_execution_modes(
            r#"
            function Constructor(executor) {
                executor(function(value) {$262.gc(); print(value.length);}, function(error) {print(error.name);});
                return {rank:42};
            }
            Object.defineProperty(Constructor, 'resolve', {get() {$262.gc(); return function(value) {return value;};}});
            print(Promise.all.call(Constructor, []).rank);
            "#,
            &["0", "42"],
        );
    }

    #[test]
    fn regression_promise_then_observes_state_after_species_and_capability_effects() {
        assert_output_in_execution_modes(
            r#"
            var ordinal = 0;
            for (var phase of ['constructor', 'species', 'capability']) {
                for (var rejected of [false, true]) {
                    (function(phase, rejected, index) {
                        var settle;
                        var source = new Promise(function(resolve, reject) {settle = rejected ? reject : resolve;});
                        var value = {rank: index};
                        function effect() {
                            print('effect:' + index); $262.gc(); settle(value);
                            Object.defineProperty(source, 'constructor', {value:Promise, writable:true, configurable:true});
                            source.then(function(actual) {print('inner:' + index + ':' + (actual === value));},
                                function(actual) {print('inner:' + index + ':' + (actual === value));});
                        }
                        if (phase === 'constructor') {
                            Object.defineProperty(source, 'constructor', {get() {effect(); return Promise;}, configurable:true});
                        } else if (phase === 'species') {
                            source.constructor = {get [Symbol.species]() {effect(); return Promise;}};
                        } else {
                            source.constructor = {[Symbol.species]: function(executor) {effect(); return new Promise(executor);}};
                        }
                        source.then(function(actual) {print('outer:' + index + ':' + (actual === value)); return index;},
                            function(actual) {print('outer:' + index + ':' + (actual === value)); return index;})
                            .then(function(actual) {print('next:' + actual);});
                    })(phase, rejected, ordinal++);
                }
            }
            "#,
            &[
                "effect:0",
                "effect:1",
                "effect:2",
                "effect:3",
                "effect:4",
                "effect:5",
                "inner:0:true",
                "outer:0:true",
                "inner:1:true",
                "outer:1:true",
                "inner:2:true",
                "outer:2:true",
                "inner:3:true",
                "outer:3:true",
                "inner:4:true",
                "outer:4:true",
                "inner:5:true",
                "outer:5:true",
                "next:0",
                "next:1",
                "next:2",
                "next:3",
                "next:4",
                "next:5",
            ],
        );
    }

    #[test]
    fn regression_promise_default_species_uses_method_realm_intrinsics() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm().global;
            var home = foreign.Promise, local = Promise;
            var foreignThen = home.prototype.then, localThen = local.prototype.then;
            home.prototype.constructor = function Poison() {throw 'prototype constructor';};
            foreign.Promise = function Poison() {throw 'global constructor';};
            globalThis.Promise = function Poison() {throw 'local global constructor';};
            for (var constructor of [undefined, {[Symbol.species]: null}, {get [Symbol.species]() {$262.gc(); return undefined;}}]) {
                var source = local.resolve(42); source.constructor = constructor;
                var result = foreignThen.call(source);
                print(Object.getPrototypeOf(result) === home.prototype);
                var other = home.resolve(43); other.constructor = constructor;
                print(Object.getPrototypeOf(localThen.call(other)) === local.prototype);
            }
            "#,
            &["true", "true", "true", "true", "true", "true"],
        );
    }

    #[test]
    fn regression_promise_finally_selects_species_before_then_for_every_handler() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm().global;
            var finallyMethod = foreign.Promise.prototype.finally;
            for (var handler of [undefined, 1, function() {}]) {
                var order = [];
                var source = {
                    get constructor() {order.push('constructor'); $262.gc(); return {
                        get [Symbol.species]() {order.push('species'); $262.gc(); return undefined;}
                    };},
                    get then() {order.push('then'); $262.gc(); return function(a, b) {
                        order.push('call'); print(handler === undefined || handler === 1 ? a === handler && b === handler : typeof a === 'function' && typeof b === 'function');
                        return 42;
                    };}
                };
                print(finallyMethod.call(source, handler)); print(order.join(','));
            }
            var reads = 0, marker = {};
            var invalid = {constructor: {[Symbol.species]: () => {}}, get then() {reads++; throw marker;}};
            try {finallyMethod.call(invalid);} catch (error) {print(error instanceof foreign.TypeError);}
            print(reads);
            var abrupt = {get constructor() {throw marker;}, get then() {reads++;}};
            try {finallyMethod.call(abrupt, null);} catch (error) {print(error === marker);}
            print(reads);
            for (var primitive of [1, true, 's', Symbol(), 1n]) {
                try {finallyMethod.call(primitive);} catch (error) {print(error instanceof foreign.TypeError);}
            }
            "#,
            &[
                "true",
                "42",
                "constructor,species,then,call",
                "true",
                "42",
                "constructor,species,then,call",
                "true",
                "42",
                "constructor,species,then,call",
                "true",
                "0",
                "true",
                "0",
                "true",
                "true",
                "true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_foreign_promise_installs_distinct_methods_and_descriptors() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm().global;
            var constructor = foreign.Promise;
            print(constructor !== Promise);
            var prototype = Object.getOwnPropertyDescriptor(constructor, 'prototype');
            print(prototype.writable + ',' + prototype.enumerable + ',' + prototype.configurable);
            var owners = [[constructor, 'resolve', 1], [constructor, 'reject', 1],
                [constructor, 'all', 1], [constructor, 'race', 1], [constructor, 'allSettled', 1],
                [constructor, 'any', 1], [constructor, 'try', 1], [constructor, 'withResolvers', 0],
                [constructor.prototype, 'then', 2], [constructor.prototype, 'catch', 1],
                [constructor.prototype, 'finally', 1]];
            for (var entry of owners) {
                var owner = entry[0], name = entry[1];
                var d = Object.getOwnPropertyDescriptor(owner, name);
                var local = owner === constructor ? Promise : Promise.prototype;
                print(d.value.name === name && d.value.length === entry[2] && d.value !== local[name]
                    && d.writable && !d.enumerable && d.configurable);
            }
            var getter = Object.getOwnPropertyDescriptor(constructor, Symbol.species).get;
            print(getter.call(constructor) === constructor);
            print(constructor.prototype.constructor === constructor);
            print(Object.prototype.toString.call(constructor.prototype));
            try {constructor.any.call({}, []);} catch (error) {print(error instanceof foreign.TypeError);}
            try {constructor.prototype.then.call({});} catch (error) {print(error instanceof foreign.TypeError);}
            $262.gc();
            print(constructor.prototype.then !== Promise.prototype.then);
            "#,
            &[
                "true",
                "false,false,false",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "[object Promise]",
                "true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_promise_any_uses_intrinsics_after_global_replacement() {
        assert_output_in_execution_modes(
            r#"
            var home = AggregateError.prototype, arrayHome = Array.prototype;
            globalThis.AggregateError = {prototype: {poison: true}};
            var reason = {rank: 42};
            class Immediate extends Promise {
                static resolve(value) {return {then(resolve, reject) {reject(value);}};}
            }
            function inspect(error) {
                $262.gc();
                var d = Object.getOwnPropertyDescriptor(error, 'errors');
                return [Error.isError(error), Object.getPrototypeOf(error) === home,
                    Object.getPrototypeOf(error.errors) === arrayHome,
                    d.writable, d.enumerable, d.configurable,
                    error.errors.length, error.errors.length === 0 || error.errors[0] === reason].join(',');
            }
            Promise.all([
                Promise.any([]).catch(inspect),
                Promise.any([Promise.reject(reason)]).catch(inspect),
                Promise.any.call(Immediate, [reason]).catch(inspect)
            ]).then(function(results) {for (var result of results) print(result);});
            "#,
            &[
                "true,true,true,true,false,true,0,true",
                "true,true,true,true,false,true,1,true",
                "true,true,true,true,false,true,1,true",
            ],
        );
    }

    #[test]
    fn regression_promise_any_borrowing_preserves_error_and_array_realms() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm().global;
            var localError = AggregateError.prototype, localArray = Array.prototype;
            var foreignError = foreign.AggregateError.prototype, foreignArray = foreign.Array.prototype;
            foreign.AggregateError = {prototype: {poison: true}};
            globalThis.AggregateError = {prototype: {poison: true}};
            function inspect(error, errorHome, arrayHome) {
                $262.gc();
                return [Error.isError(error), Object.getPrototypeOf(error) === errorHome,
                    Object.getPrototypeOf(error.errors) === arrayHome, error.errors.join(',')].join(':');
            }
            function foreignCheck(error) {return inspect(error, foreignError, foreignArray);}
            function localCheck(error) {return inspect(error, localError, localArray);}
            Promise.all([
                foreign.Promise.any.call(Promise, []).catch(foreignCheck),
                foreign.Promise.any.call(Promise, [Promise.reject(1), Promise.reject(2)]).catch(foreignCheck),
                Promise.any.call(foreign.Promise, []).catch(localCheck),
                Promise.any.call(foreign.Promise, [foreign.Promise.reject(3)]).catch(localCheck)
            ]).then(function(results) {for (var result of results) print(result);});
            "#,
            &[
                "true:true:true:",
                "true:true:true:1,2",
                "true:true:true:",
                "true:true:true:3",
            ],
        );
    }

    #[test]
    fn regression_error_brand_is_not_a_guest_property() {
        assert_output_in_execution_modes(
            r#"
            var marker = '\0rqj:error-brand';
            var forged = {[marker]: true};
            print(Error.isError(forged));
            print(Object.prototype.toString.call(forged));
            var error = new Error('real');
            print(Object.hasOwn(error, marker));
            error[marker] = false;
            print(Error.isError(error));
            print(Object.prototype.toString.call(error));
            delete error[marker];
            print(Error.isError(error));
            print(Error.isError(Object.create(error)));
            print(Error.isError(Error.prototype));
            var traps = 0;
            var proxy = new Proxy(error, {get() {traps++; throw 'get';}, getOwnPropertyDescriptor() {traps++; throw 'descriptor';}});
            print(Error.isError(proxy));
            var revoked = Proxy.revocable(error, {}); revoked.revoke();
            print(Error.isError(revoked.proxy)); print(traps);
            class Custom extends Error {}
            var custom = new Custom('subclass');
            Object.setPrototypeOf(custom, null);
            $262.gc(); print(Error.isError(custom)); print(Error.isError(error));
            error[Symbol.toStringTag] = 'Custom';
            print(Object.prototype.toString.call(error));
            for (var constructor of [Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError])
                print(Error.isError(new constructor('native')));
            print(Error.isError(new AggregateError([], 'aggregate')));
            print(Error.isError(new SuppressedError(1, 2, 'suppressed')));
            try {null.value;} catch (caught) {print(Error.isError(caught));}
            "#,
            &[
                "false",
                "[object Object]",
                "false",
                "true",
                "[object Error]",
                "true",
                "false",
                "false",
                "false",
                "false",
                "0",
                "true",
                "true",
                "[object Custom]",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
                "true",
            ],
        );
    }

    #[test]
    fn regression_error_brand_survives_realms_and_aggregate_jobs() {
        assert_output_in_execution_modes(
            r#"
            var foreign = $262.createRealm().global;
            var error = new foreign.Error('foreign');
            print(Error.isError(error)); print(foreign.Error.isError(new Error('local')));
            print(Error.isError(new Proxy(error, {})));
            var stack = Object.getOwnPropertyDescriptor(Error.prototype, 'stack').get;
            print(stack.call({'\0rqj:error-brand':true}) === undefined);
            error['\0rqj:error-brand'] = false;
            $262.gc(); print(stack.call(error));
            Promise.any([]).catch(function(error) {
                print(Error.isError(error)); print(Object.hasOwn(error, '\0rqj:error-brand'));
                error['\0rqj:error-brand'] = false;
                $262.gc(); print(Error.isError(error)); print(Object.prototype.toString.call(error));
            });
            for (var value of [Object('s'), Object(1), Object(true), Object(1n), Object(Symbol()), [], function() {}])
                print(Error.isError(value));
            "#,
            &[
                "true",
                "true",
                "false",
                "true",
                "Error: foreign",
                "false",
                "false",
                "false",
                "false",
                "false",
                "false",
                "false",
                "true",
                "false",
                "true",
                "[object Error]",
            ],
        );
    }

    #[test]
    fn regression_error_stack_setter_uses_intrinsic_home_and_records() {
        assert_output_in_execution_modes(
            r#"
            var home = Error.prototype;
            var setter = Object.getOwnPropertyDescriptor(home, 'stack').set;
            var reads = 0;
            Object.defineProperty(globalThis, 'Error', {get() {reads++; $262.gc(); throw 'replaced Error';}, configurable:true});
            var poison = Object.create(null);
            poison.get = function() {throw 'inherited descriptor getter';};
            poison.configurable = true;
            Object.defineProperty(Object.prototype, 'get', poison);
            var target = {get inherited() {throw 'unused';}};
            print(setter.call(target, 'saved') === undefined);
            var d = Object.getOwnPropertyDescriptor(target, 'stack');
            print(d.value); print(d.writable); print(d.enumerable); print(d.configurable);
            try {setter.call(home, 'invalid');} catch (error) {print(error instanceof TypeError);}
            try {setter.call({}, 1);} catch (error) {print(error instanceof TypeError);}
            print(reads);
            delete Object.prototype.get;
            "#,
            &["true", "saved", "true", "true", "true", "true", "true", "0"],
        );
    }

    #[test]
    fn regression_error_stack_setter_shares_assignment_and_realm_identity() {
        assert_output_in_execution_modes(
            r#"
            var setter = Object.getOwnPropertyDescriptor(Error.prototype, 'stack').set;
            var data = {};
            Object.defineProperty(data, 'stack', {value:'old', writable:true, configurable:false});
            var observed;
            var accessor = {set stack(v) {$262.gc(); observed = v;}};
            var emptyAccessor = {};
            Object.defineProperty(emptyAccessor, 'stack', {get:undefined, set:undefined});
            var poison = Object.create(null);
            poison.get = function() {throw 'inherited descriptor getter';};
            poison.configurable = true;
            Object.defineProperty(Object.prototype, 'get', poison);
            setter.call(data, 'updated'); print(data.stack);
            print(Object.getOwnPropertyDescriptor(data, 'stack').enumerable);
            setter.call(accessor, 'accessor'); print(observed);
            try {setter.call(emptyAccessor, 'invalid');} catch (error) {print(error instanceof TypeError);}
            delete Object.prototype.get;
            var trace = [];
            var proxy = new Proxy({}, {
                getOwnPropertyDescriptor(t,k) {$262.gc(); trace.push('get:' + k); return Reflect.getOwnPropertyDescriptor(t,k);},
                defineProperty(t,k,d) {$262.gc(); trace.push('define:' + k + ':' + Object.keys(d)); return Reflect.defineProperty(t,k,d);}
            });
            setter.call(proxy, 'fresh' + '-stack'); print(proxy.stack); print(trace.join(';'));
            var foreign = $262.createRealm().global;
            var foreignHome = foreign.Error.prototype;
            var foreignSetter = Object.getOwnPropertyDescriptor(foreignHome, 'stack').set;
            Object.defineProperty(foreign, 'Error', {get() {$262.gc(); throw 'foreign Error';}, configurable:true});
            try {foreignSetter.call(foreignHome, 'invalid');} catch (error) {print(error instanceof foreign.TypeError);}
            var foreignTarget = new foreign.Object();
            foreignSetter.call(foreignTarget, 'foreign'); print(foreignTarget.stack);
            "#,
            &[
                "updated",
                "false",
                "accessor",
                "true",
                "fresh-stack",
                "get:stack;define:stack:value,writable,enumerable,configurable",
                "true",
                "foreign",
            ],
        );
    }

    #[test]
    fn regression_integrity_records_preserve_proxy_order_and_isolation() {
        assert_output_in_execution_modes(
            r#"
            var trace = [];
            var target = {answer:42};
            var sealed = new Proxy(target, {
                preventExtensions(t) {trace.push('prevent'); return Reflect.preventExtensions(t);},
                ownKeys(t) {trace.push('keys'); return Reflect.ownKeys(t);},
                getOwnPropertyDescriptor() {throw 'unexpected descriptor read';},
                defineProperty(t, k, d) {trace.push('define:' + k + ':' + Object.keys(d)); Object.setPrototypeOf(d, null); return Reflect.defineProperty(t, k, d);}
            });
            print(Object.seal(sealed) === sealed);
            print(trace.join(';'));
            trace = [];
            target = {answer:43, get accessor() {return 44;}};
            var frozen = new Proxy(target, {
                preventExtensions(t) {trace.push('prevent'); return Reflect.preventExtensions(t);},
                ownKeys(t) {trace.push('keys'); return Reflect.ownKeys(t);},
                getOwnPropertyDescriptor(t, k) {trace.push('get:' + k); var d = Reflect.getOwnPropertyDescriptor(t, k); Object.setPrototypeOf(d, null); return d;},
                defineProperty(t, k, d) {trace.push('define:' + k + ':' + Object.keys(d)); Object.setPrototypeOf(d, null); return Reflect.defineProperty(t, k, d);}
            });
            var poisonValue = {get() {throw 'inherited descriptor value';}, configurable:true};
            var poisonGet = {get() {throw 'inherited descriptor getter';}, configurable:true};
            Object.setPrototypeOf(poisonGet, null);
            Object.defineProperty(Object.prototype, 'value', poisonValue);
            Object.defineProperty(Object.prototype, 'get', poisonGet);
            print(Object.freeze(frozen) === frozen);
            delete Object.prototype.value;
            delete Object.prototype.get;
            print(trace.join(';'));
            print(target.answer);
            print(target.accessor);
            print(Object.isFrozen(target));
            "#,
            &[
                "true",
                "prevent;keys;define:answer:configurable",
                "true",
                "prevent;keys;get:answer;define:answer:writable,configurable;get:accessor;define:accessor:configurable",
                "43",
                "44",
                "true",
            ],
        );
    }

    #[test]
    fn regression_integrity_key_snapshot_survives_collecting_traps() {
        assert_output_in_execution_modes(
            r#"
            for (var freeze of [false, true]) {
                var trace = [];
                var target = {alpha:42, beta:43};
                var proxy = new Proxy(target, {
                    preventExtensions(t) {$262.gc(); return Reflect.preventExtensions(t);},
                    ownKeys() {return ['al' + 'pha', 'be' + 'ta'];},
                    getOwnPropertyDescriptor(t,k) {$262.gc(); return Reflect.getOwnPropertyDescriptor(t,k);},
                    defineProperty(t,k,d) {$262.gc(); trace.push(k); return Reflect.defineProperty(t,k,d);}
                });
                print((freeze ? Object.freeze(proxy) : Object.seal(proxy)) === proxy);
                print(trace.join(','));
                print(freeze ? Object.isFrozen(proxy) : Object.isSealed(proxy));
                print(target.alpha + target.beta);
            }
            "#,
            &[
                "true",
                "alpha,beta",
                "true",
                "85",
                "true",
                "alpha,beta",
                "true",
                "85",
            ],
        );
    }

    #[test]
    fn regression_integrity_observes_disappearing_snapshot_properties() {
        assert_output_in_execution_modes(
            r#"
            for (var freeze of [false, true]) {
                var target = {alpha:42, beta:43};
                Object.defineProperty(target, 'alpha', {configurable:false, writable:!freeze});
                Object.preventExtensions(target);
                var trace = [];
                var proxy = new Proxy(target, {
                    isExtensible(t) {$262.gc(); trace.push('extensible'); return Reflect.isExtensible(t);},
                    ownKeys(t) {$262.gc(); trace.push('keys'); return Reflect.ownKeys(t);},
                    getOwnPropertyDescriptor(t,k) {$262.gc(); trace.push(k); delete t.beta; return Reflect.getOwnPropertyDescriptor(t,k);}
                });
                print(freeze ? Object.isFrozen(proxy) : Object.isSealed(proxy));
                print(trace.join(','));
                target = {alpha:42, beta:43};
                trace = [];
                proxy = new Proxy(target, {
                    defineProperty(t,k,d) {$262.gc(); trace.push(k); delete t.beta; return Reflect.defineProperty(t,k,d);}
                });
                try {print((freeze ? Object.freeze(proxy) : Object.seal(proxy)) === proxy);}
                catch (error) {print(error instanceof TypeError);}
                print(trace.join(','));
                print(Object.isExtensible(target));
                print(Object.getOwnPropertyDescriptor(target, 'alpha').configurable);
                print('beta' in target);
            }
            var extensible = new Proxy({}, {ownKeys() {throw 'unexpected ownKeys';}});
            print(Object.isFrozen(extensible));
            print(Object.isSealed(extensible));
            "#,
            &[
                "true",
                "extensible,keys,alpha,beta",
                "true",
                "alpha,beta",
                "false",
                "false",
                "false",
                "true",
                "extensible,keys,alpha,beta",
                "true",
                "alpha",
                "false",
                "false",
                "false",
                "false",
                "false",
            ],
        );
    }

    #[test]
    fn regression_integrity_namespace_records_preserve_live_exports() {
        const DEPENDENCY: &str = "export let answer = 42; export let method = function() {return answer;}; export function change() {answer = 43; method = function() {return answer + 1;};}";
        const SOURCE: &str = r#"
            import * as ns from './integrity-dependency.mjs';
            var poison = Object.create(null);
            poison.get = function() {throw 'inherited descriptor getter';};
            poison.configurable = true;
            Object.defineProperty(Object.prototype, 'get', poison);
            print(Object.seal(ns) === ns);
            print(Object.isSealed(ns));
            print(Object.isFrozen(ns));
            try {Object.freeze(ns);}
            catch (error) {print(error instanceof TypeError);}
            delete Object.prototype.get;
            var inherited = Object.create(ns);
            function read(value) {return value.answer;}
            function call(value) {return value.method();}
            for (var index = 0; index < 5; index++) {read(ns); read(inherited); call(ns); call(inherited);}
            print(ns.answer);
            ns.change();
            print(ns.answer);
            print(read(ns)); print(read(inherited));
            print(call(ns)); print(call(inherited));
            print(Object.defineProperty(ns, 'answer', {value:43}) === ns);
            print(Reflect.defineProperty(ns, 'answer', {value:42}));
            print(Object.getOwnPropertyDescriptor(ns, 'answer').value);
            print(Object.getOwnPropertyDescriptor(ns, 'answer').writable);
            print(Object.getOwnPropertyDescriptor(ns, Symbol.toStringTag).writable);
        "#;
        struct ModuleCapture(Capture);
        impl Host for ModuleCapture {
            fn write_line(&mut self, text: &str) {
                self.0.write_line(text);
            }
            fn clock_millis(&mut self) -> f64 {
                0.0
            }
            fn resolve_dynamic_import(
                &mut self,
                _: &str,
                specifier: &str,
            ) -> Result<Option<crate::host::ModuleSource>, String> {
                assert_eq!(specifier, "./integrity-dependency.mjs");
                Ok(Some(crate::host::ModuleSource {
                    name: "integrity-dependency.mjs".into(),
                    source: DEPENDENCY.into(),
                    bytes: vec![],
                }))
            }
        }
        for compile in [
            Engine::specialize_module as fn(&str, &str) -> _,
            Engine::specialize_module_unspecialized,
        ] {
            let view = Capture::default();
            let mut runtime = Runtime::new(ModuleCapture(view.clone()));
            let program = compile(SOURCE, "integrity-namespace.mjs").unwrap();
            runtime.execute(&program).unwrap();
            assert_eq!(
                view.0.borrow().as_slice(),
                &[
                    "true", "true", "false", "true", "42", "43", "43", "43", "44", "44", "true",
                    "false", "43", "true", "false"
                ]
            );
        }
    }

    #[test]
    fn regression_legacy_accessor_records_preserve_order_and_roots() {
        assert_output_in_execution_modes(
            r#"
            var reads = 0;
            for (var method of [Object.prototype.__defineGetter__, Object.prototype.__defineSetter__]) {
                try {method.call({}, {toString() {reads++; return 'entry';}}, undefined); print(false);}
                catch (error) {print(error instanceof TypeError);}
                print(method.call(1, {toString() {$262.gc(); return 'entry';}}, function() {}) === undefined);
            }
            print(reads);
            var getter = Object.prototype.__defineGetter__, setter = Object.prototype.__defineSetter__;
            var target = {}, value, keys = [];
            var proxy = new Proxy(target, {defineProperty(object,key,descriptor) {
                keys.push(Object.keys(descriptor).join(',')); $262.gc();
                Object.setPrototypeOf(descriptor,null); return Reflect.defineProperty(object,key,descriptor);
            }});
            Object.defineProperty(Object.prototype, 'value', {configurable:true, get() {$262.gc(); throw 'inherited value';}});
            try {
                getter.call(proxy,'entry',function() {return 42;});
                setter.call(proxy,'entry',function(next) {value = next;});
                print(proxy.entry); proxy.entry = 43; print(value);
            } finally {delete Object.prototype.value;}
            print(keys.join(';'));
            "#,
            &[
                "true",
                "true",
                "true",
                "true",
                "0",
                "42",
                "43",
                "get,enumerable,configurable;set,enumerable,configurable",
            ],
        );
    }

    #[test]
    fn regression_internal_data_records_bypass_inherited_descriptor_fields() {
        assert_output_in_execution_modes(
            r#"
            var constructorSetter = Object.getOwnPropertyDescriptor(Iterator.prototype,'constructor').set;
            var tagSetter = Object.getOwnPropertyDescriptor(Iterator.prototype,Symbol.toStringTag).set;
            var symbol = Symbol('field'), ordinary, derived, iterator = {}, binding;
            Object.defineProperty(Object.prototype,'get',{configurable:true,get() {$262.gc(); throw 'inherited get';}});
            try {
                class Ordinary {[symbol] = {rank:42}; plain = 43;}
                ordinary = new Ordinary();
                class Base {constructor() {return new Proxy({}, {defineProperty(object,key,descriptor) {
                    $262.gc(); object[key] = descriptor.value; return true;
                }});}}
                class Derived extends Base {entry = {rank:44};}
                derived = new Derived();
                constructorSetter.call(iterator,{rank:45}); tagSetter.call(iterator,'custom');
                (0,eval)('var internalRecordBinding = 46;'); binding = internalRecordBinding;
            } finally {delete Object.prototype.get; delete globalThis.internalRecordBinding;}
            print(ordinary[symbol].rank); print(ordinary.plain); print(derived.entry.rank);
            print(iterator.constructor.rank); print(iterator[Symbol.toStringTag]); print(binding);
            var descriptor = Object.getOwnPropertyDescriptor(iterator,'constructor');
            print(descriptor.writable); print(descriptor.enumerable); print(descriptor.configurable);
            "#,
            &[
                "42", "43", "44", "45", "custom", "46", "true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_raw_json_brand_is_not_a_guest_property() {
        assert_output_in_execution_modes(
            r#"
            var forged = {rawJSON:'not-json', ['\0rqj:raw-json']:true};
            print(JSON.isRawJSON(forged)); print(JSON.stringify(forged).startsWith('{'));
            var raw = JSON.rawJSON('42'); $262.gc();
            print(JSON.isRawJSON(raw)); print(JSON.isRawJSON(Object.create(raw)));
            print(JSON.isRawJSON(new Proxy(raw,{})));
            print(JSON.stringify(new Proxy(raw,{}))); print(JSON.stringify(raw));
            "#,
            &[
                "false",
                "true",
                "true",
                "false",
                "false",
                "{\"rawJSON\":\"42\"}",
                "42",
            ],
        );
    }

    #[test]
    fn regression_json_internal_definitions_ignore_inherited_descriptor_fields() {
        assert_output_in_execution_modes(
            r#"
            var parsed, raw;
            Object.defineProperty(Object.prototype, 'get', {configurable:true, get() {$262.gc(); throw 'inherited descriptor';}});
            try {
                parsed = JSON.parse('{"entry":1}', function(key,value) {$262.gc(); return key === 'entry' ? {rank:42} : value;});
                raw = JSON.rawJSON('43');
            } finally {delete Object.prototype.get;}
            print(parsed.entry.rank); print(raw.rawJSON);
            var descriptor = Object.getOwnPropertyDescriptor(raw,'rawJSON');
            print(descriptor.writable); print(descriptor.enumerable); print(descriptor.configurable);
            print(Object.isFrozen(raw)); print(Object.getPrototypeOf(raw) === null);
            print(Object.keys(raw).join(',')); print(JSON.stringify(raw));
            "#,
            &[
                "42", "43", "false", "true", "false", "true", "true", "rawJSON", "43",
            ],
        );
    }

    #[test]
    fn regression_json_reviver_ignores_rejection_and_preserves_throw() {
        assert_output_in_execution_modes(
            r#"
            var value = JSON.parse('{"first":1,"second":2}', function(key,value) {
                if (key === 'first') {delete this.second; Object.preventExtensions(this);}
                $262.gc(); return key === 'second' ? {rank:42} : value;
            });
            print(value.first); print(Object.hasOwn(value,'second'));
            var calls = 0;
            value = JSON.parse('{"first":0,"target":{}}', function(key,value) {
                if (key === 'first') this.target = new Proxy({entry:1}, {defineProperty(object,key,descriptor) {
                    $262.gc(); calls++; print(descriptor.value.rank); return false;
                }});
                return key === 'entry' ? {rank:43} : value;
            });
            print(calls); print(value.target.entry);
            var marker = {rank:44};
            try {JSON.parse('{"first":0,"target":{}}', function(key,value) {
                if (key === 'first') this.target = new Proxy({entry:1}, {defineProperty() {$262.gc(); throw marker;}});
                return value;
            });}
            catch (error) {print(error === marker);}
            "#,
            &["1", "false", "43", "1", "1", "true"],
        );
    }

    #[test]
    fn regression_define_properties_collects_before_applying() {
        assert_output_in_execution_modes(
            r#"
            var target = {}, calls = 0;
            var proxy = new Proxy(target, {defineProperty() {calls++; return true;}});
            try {Object.defineProperties(proxy, {first:{value:1}, second:{get:1}});}
            catch (error) {print(error instanceof TypeError);}
            print(calls); print(Object.keys(target).length);
            var events = [];
            var input = {
                get first() {events.push('get-first'); return {get value() {events.push('value-first'); $262.gc(); return {rank:42};}, configurable:true};},
                get second() {events.push('get-second'); $262.gc(); return {get value() {events.push('value-second'); return 43;}, configurable:true};}
            };
            target = {};
            proxy = new Proxy(target, {defineProperty(object,key,descriptor) {
                events.push('define-' + key); $262.gc(); return Reflect.defineProperty(object,key,descriptor);
            }});
            print(Object.defineProperties(proxy,input) === proxy);
            print(events.join(',')); print(target.first.rank); print(target.second);
            "#,
            &[
                "true",
                "0",
                "0",
                "true",
                "get-first,value-first,get-second,value-second,define-first,define-second",
                "42",
                "43",
            ],
        );
    }

    #[test]
    fn regression_match_all_orders_species_and_preserves_strings_and_setters() {
        assert_output_in_execution_modes(
            r#"
            var log=[];
            function matcherFactory() {var index=0;return {
                get exec() {$262.gc();log.push('exec:get');return execute;},
                get lastIndex() {$262.gc();log.push('index:get');return {[Symbol.toPrimitive](hint) {$262.gc();log.push('index:'+hint);return index;}};},
                set lastIndex(value) {$262.gc();log.push('index:set:'+value);index=value;}
            };}
            function execute(input) {$262.gc();log.push('exec:'+input.charCodeAt(0));return resultFactory(input);}
            function resultFactory(input) {return {tag:42,input:input,get 0() {$262.gc();log.push('match:get');return {[Symbol.toPrimitive](hint) {$262.gc();log.push('match:'+hint);return '';}};}};}
            function speciesFactory() {return function Species(receiver,flags) {$262.gc();log.push('construct:'+flags.charCodeAt(0));return matcherFactory();};}
            var receiver={
                get constructor() {$262.gc();log.push('constructor');return {get [Symbol.species]() {$262.gc();log.push('species');return speciesFactory();}};},
                get flags() {$262.gc();log.push('flags:get');return {[Symbol.toPrimitive](hint) {$262.gc();log.push('flags:'+hint);return String.fromCharCode(55296)+'g';}};},
                get lastIndex() {$262.gc();log.push('lastIndex:get');return {[Symbol.toPrimitive](hint) {$262.gc();log.push('lastIndex:'+hint);return 0;}};}
            };
            var input={[Symbol.toPrimitive](hint) {$262.gc();log.push('input:'+hint);return String.fromCharCode(55296,97);}};
            var iterator=RegExp.prototype[Symbol.matchAll].call(receiver,input);
            var result=iterator.next();
            print(result.value.tag);
            print(result.value.input.charCodeAt(0));
            print(log.join(','));
            "#,
            &[
                "42",
                "55296",
                "input:string,constructor,species,flags:get,flags:string,construct:55296,lastIndex:get,lastIndex:number,index:set:0,exec:get,exec:55296,match:get,match:string,index:get,index:number,index:set:1",
            ],
        );
    }

    #[test]
    fn regression_match_all_observes_strict_set_errors_and_species_validation() {
        assert_output_in_execution_modes(
            r#"
            for (var phase of ['create','advance']) {
                var marker={},writes=0;
                function Species() {return {exec() {$262.gc();return {0:''};},get lastIndex() {return 0;},set lastIndex(value) {$262.gc();writes++;if (phase==='create'||writes===2) throw marker;}};}
                var receiver={constructor:{[Symbol.species]:Species},flags:'g',lastIndex:0};
                try {var iterator=RegExp.prototype[Symbol.matchAll].call(receiver,'a');iterator.next();print(false);} catch (error) {print(error===marker);}
                print(writes);
            }
            var reads=0;
            for (var method of [Symbol.matchAll,Symbol.split]) {
                var receiver={constructor:{[Symbol.species]:()=>({})},get flags() {reads++;throw 'flags';}};
                try {RegExp.prototype[method].call(receiver,'a');print(false);} catch (error) {print(error instanceof TypeError);}
            }
            print(reads);
            var matcher={};Object.defineProperty(matcher,'lastIndex',{value:0,writable:false});
            var receiver={constructor:{[Symbol.species]:function(){return matcher;}},flags:'g',lastIndex:0};
            try {RegExp.prototype[Symbol.matchAll].call(receiver,'a');print(false);} catch (error) {print(error instanceof TypeError);}
            for (var flags of ['g','gu','gv','']) {
                var writes=[],matchReads=0;
                function Species() {return {exec() {return {get 0() {matchReads++;return '';}};},get lastIndex() {return 0;},set lastIndex(value) {$262.gc();writes.push(value);}};}
                var receiver={constructor:{[Symbol.species]:Species},flags:flags,lastIndex:0};
                RegExp.prototype[Symbol.matchAll].call(receiver,String.fromCodePoint(128512)).next();
                print(writes.join(',')+':'+matchReads);
            }
            "#,
            &[
                "true", "1", "true", "2", "true", "true", "0", "true", "0,1:1", "0,2:1", "0,2:1", "0:0",
            ],
        );
    }

    #[test]
    fn regression_array_iterator_advances_before_get_and_reentrant_next() {
        assert_output_in_execution_modes(
            r#"
            for (var method of ['values','entries']) {
                var reads=0,iterator;
                var source={length:2,get 0() {$262.gc();reads++;if (reads===1) throw 'item';return 10;},1:20};
                iterator=Array.prototype[method].call(source);
                try {iterator.next();} catch (error) {print(error);}
                var value=iterator.next().value;
                print(method==='entries'?value[0]+':'+value[1]:value);
                print(reads);
                var nested,entered=false;
                source={length:2,get 0() {$262.gc();if (!entered) {entered=true;nested=iterator.next().value;}return 10;},1:20};
                iterator=Array.prototype[method].call(source);
                value=iterator.next().value;
                print(method==='entries'?value[0]+':'+value[1]:value);
                print(method==='entries'?nested[0]+':'+nested[1]:nested);
                print(iterator.next().done);
            }
            "#,
            &[
                "item", "20", "1", "10", "20", "true", "item", "1:20", "1", "0:10", "1:20", "true",
            ],
        );
    }

    #[test]
    fn regression_iterator_step_rejects_primitive_before_done_access() {
        assert_output_in_execution_modes(
            r#"
            var reads = 0;
            Object.defineProperty(Number.prototype,'done',{get:function() {reads++;return true;}});
            for (var Consumer of [Map, Set, WeakMap, WeakSet]) {
                var iterator = Iterator.from({next:function() {$262.gc();return 42;}});
                try {new Consumer(iterator); print(false);} catch (error) {print(error instanceof TypeError);}
            }
            print(reads);
            "#,
            &["true", "true", "true", "true", "0"],
        );
    }

    #[test]
    fn regression_bound_has_instance_dispatches_the_target_method() {
        assert_output_in_execution_modes(
            r#"
            function C() {}
            var calls = 0, value = {tag:42};
            Object.defineProperty(C, Symbol.hasInstance, {configurable:true, value:function(v) {
                $262.gc(); calls++; return this===C && v===value;
            }});
            var bound = C.bind(null).bind(null);
            print(value instanceof bound);
            print(Function.prototype[Symbol.hasInstance].call(bound,value));
            print(calls);
            Object.defineProperty(C, Symbol.hasInstance, {get:function() {$262.gc(); throw value;}});
            try {value instanceof bound; print(false);} catch (error) {print(error===value);}
            "#,
            &["true", "true", "2", "true"],
        );
    }

    #[test]
    fn regression_binary_operand_coercion_preserves_order_and_hints() {
        assert_output_in_execution_modes(
            r#"
            var events=[];
            var left={[Symbol.toPrimitive](hint) {events.push('left:'+hint);return 41;}};
            var right={[Symbol.toPrimitive](hint) {events.push('right:'+hint);$262.gc();return 1;}};
            print(left+right);print(events.join(','));events=[];
            print(left-right);print(events.join(','));events=[];
            print(left<right);print(events.join(','));
            "#,
            &[
                "42",
                "left:default,right:default",
                "40",
                "left:number,right:number",
                "false",
                "left:number,right:number",
            ],
        );
    }

    #[test]
    fn regression_binary_coercion_keeps_fresh_first_primitive() {
        assert_output_in_execution_modes(
            r#"
            function pair(bigint) {return [
                {[Symbol.toPrimitive](hint) {return bigint?BigInt(42):String.fromCharCode(97,98);}},
                {[Symbol.toPrimitive](hint) {$262.gc();return bigint?BigInt(7):String.fromCharCode(99);}}
            ];}
            for (var bigint of [false,true]) {
                var values=pair(bigint);print(values[0]<values[1]);
                values=pair(bigint);print(values[0]<=values[1]);
                values=pair(bigint);print(values[0]>values[1]);
                values=pair(bigint);print(values[0]>=values[1]);
                values=pair(bigint);print(values[0]+values[1]);
            }
            function left() {return {[Symbol.toPrimitive]() {return BigInt(42);}};}
            function right() {return {[Symbol.toPrimitive]() {$262.gc();return BigInt(7);}};}
            print(left()-right());print(left()*right());print(left()/right());print(left()%right());
            print(left()|right());print(left()^right());print(left()&right());print(left()<<right());print(left()>>right());
            print(String.fromCharCode(97,98)=={[Symbol.toPrimitive]() {$262.gc();return String.fromCharCode(97,99);}});
            print({[Symbol.toPrimitive]() {$262.gc();return BigInt(43);}}==BigInt(42));
            function symbol() {return {[Symbol.toPrimitive]() {return Symbol('fresh');}};}
            try {print(symbol()<right());}catch(error) {print(error instanceof TypeError);}
            try {print(symbol()+right());}catch(error) {print(error instanceof TypeError);}
            print(left()+{[Symbol.toPrimitive]() {$262.gc();return String.fromCharCode(55);}});
            print(left()**right());
            "#,
            &[
                "true",
                "true",
                "false",
                "false",
                "abc",
                "false",
                "false",
                "true",
                "true",
                "49",
                "35",
                "294",
                "6",
                "0",
                "47",
                "45",
                "2",
                "5376",
                "0",
                "false",
                "false",
                "true",
                "true",
                "427",
                "230539333248",
            ],
        );
    }

    #[test]
    fn regression_revoked_proxy_keeps_installed_call_and_construct_methods() {
        assert_output_in_execution_modes(
            r#"
            for (var target of [{},()=>42,function(){},class C{},async()=>42,BigInt,Symbol,Function.prototype]) {
                var r=Proxy.revocable(target,{});r.revoke();$262.gc();
                print(typeof r.proxy);
                var outer=new Proxy(r.proxy,{apply() {return 43;},construct() {return {rank:44};}});
                try {print(Reflect.apply(outer,null,[]));}catch(error) {print(error instanceof TypeError);}
                try {print(Reflect.construct(outer,[]).rank);}catch(error) {print(error instanceof TypeError);}
            }
            var events=[],r=Proxy.revocable(function(){},{});r.revoke();
            var list={get length() {events.push('length');$262.gc();return 0;}};
            var ctor=new Proxy(function(){},{construct(target,args,newTarget) {events.push('trap');print(newTarget===r.proxy);return {rank:45};}});
            print(Reflect.construct(ctor,list,r.proxy).rank);print(events.join(','));
            events=[];try {Reflect.apply(r.proxy,null,list);}catch(error) {print(error instanceof TypeError);}print(events.join(','));
            "#,
            &[
                "object",
                "true",
                "true",
                "function",
                "43",
                "true",
                "function",
                "43",
                "44",
                "function",
                "43",
                "44",
                "function",
                "43",
                "true",
                "function",
                "43",
                "44",
                "function",
                "43",
                "44",
                "function",
                "43",
                "true",
                "true",
                "45",
                "length,trap",
                "true",
                "length",
            ],
        );
    }

    #[test]
    fn regression_revoker_consumes_one_owner_and_ignores_receiver() {
        assert_output_in_execution_modes(
            r#"
            var r=Proxy.revocable({},{}),revoke=r.revoke,proxy=r.proxy;
            print(Reflect.ownKeys(r).join(','));print(Reflect.getOwnPropertyDescriptor(r,'\0rqj:proxy-revoke-target')===undefined);
            for (var key of ['proxy','revoke']) {var d=Object.getOwnPropertyDescriptor(r,key);print(d.writable&&d.enumerable&&d.configurable);}
            print(revoke.name);print(revoke.length);
            var poison=new Proxy({}, {get() {throw new Error('receiver read');}});
            print(Reflect.apply(revoke,poison,[poison]));print(revoke.call(poison));print(revoke.apply(poison,[]));
            try {Reflect.getPrototypeOf(proxy);}catch(error) {print(error instanceof TypeError);}
            print(revoke.name);print(revoke.length);
            "#,
            &[
                "proxy,revoke",
                "true",
                "true",
                "true",
                "",
                "0",
                "undefined",
                "undefined",
                "undefined",
                "true",
                "",
                "0",
            ],
        );
    }

    #[test]
    fn regression_proxy_own_keys_fallback_retains_original_target_after_revocation() {
        assert_output_in_execution_modes(
            r#"
            for (var trapped of [false,true]) {
                var symbol=Symbol('entry'),events=[];
                var target=new Proxy({first:42,[symbol]:43},{ownKeys(object) {events.push('target');$262.gc();return Reflect.ownKeys(object);}});
                var r=Proxy.revocable(target,{get ownKeys() {events.push('lookup');r.revoke();$262.gc();
                    if (!trapped) return null;
                    return function(object) {events.push('trap');$262.gc();return Reflect.ownKeys(object);};
                }});
                var keys=Reflect.ownKeys(r.proxy);print(keys[0]);print(keys[1]===symbol);print(events.join(','));
                try {Reflect.ownKeys(r.proxy);}catch(error) {print(error instanceof TypeError);}
            }
            var r=Proxy.revocable(function(){},{get construct() {r.revoke();$262.gc();return function(target,args,newTarget) {print(newTarget===r.proxy);return {rank:46};};}});
            print(Reflect.construct(r.proxy,[]).rank);
            "#,
            &[
                "first",
                "true",
                "lookup,target",
                "true",
                "first",
                "true",
                "lookup,trap,target,target",
                "true",
                "true",
                "46",
            ],
        );
    }

    #[test]
    fn regression_proxy_presence_and_delete_keep_captured_targets_after_revocation() {
        assert_output_in_execution_modes(
            r#"
            for (var remove of [false,true]) {
                for (var fallback of [false,true]) {
                    var symbol=Symbol('entry'),raw={[symbol]:42},events=[];
                    var target=new Proxy(raw,{getOwnPropertyDescriptor(object,key) {events.push('descriptor');$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);},isExtensible(object) {events.push('extensible');$262.gc();return Reflect.isExtensible(object);}});
                    var handler={};Object.defineProperty(handler,remove?'deleteProperty':'has',{get() {events.push('lookup');revocable.revoke();$262.gc();
                        if (fallback) return null;
                        return function(object,key) {events.push('trap');$262.gc();print(this===handler);print(object===target);print(key===symbol);return remove;};
                    }});
                    var revocable=Proxy.revocable(target,handler);
                    print(remove?Reflect.deleteProperty(revocable.proxy,symbol):Reflect.has(revocable.proxy,symbol));print(events.join(','));
                    try {if (remove) Reflect.deleteProperty(revocable.proxy,symbol);else Reflect.has(revocable.proxy,symbol);}catch(error) {print(error instanceof TypeError);}
                }
                events=[];target=new Proxy({}, {getOwnPropertyDescriptor() {events.push('descriptor');$262.gc();return undefined;},isExtensible() {events.push('extensible');throw new Error('unexpected');}});
                var source=new Proxy(target,{has() {return false;},deleteProperty() {return true;}});
                print(remove?Reflect.deleteProperty(source,'missing'):Reflect.has(source,'missing'));print(events.join(','));
            }
            "#,
            &[
                "true",
                "true",
                "true",
                "false",
                "lookup,trap,descriptor,extensible",
                "true",
                "true",
                "lookup",
                "true",
                "false",
                "descriptor",
                "true",
                "true",
                "true",
                "true",
                "lookup,trap,descriptor,extensible",
                "true",
                "true",
                "lookup",
                "true",
                "true",
                "descriptor",
            ],
        );
    }

    #[test]
    fn regression_proxy_presence_and_delete_coerce_keys_before_dispatch() {
        assert_output_in_execution_modes(
            r#"
            for (var remove of [false,true]) {
                var events=[],handler={};
                Object.defineProperty(handler,remove?'deleteProperty':'has',{get() {events.push('lookup');$262.gc();return function(object,key) {events.push('trap:'+key);return true;};}});
                var source=new Proxy({},handler),key={toString() {events.push('key');$262.gc();return 'entry';}};
                print(remove?Reflect.deleteProperty(source,key):key in source);print(events.join(','));
                events=[];var revoked=Proxy.revocable({},handler);revoked.revoke();
                key={toString() {events.push('key');throw {kind:'coercion'};}};
                try {if (remove) Reflect.deleteProperty(revoked.proxy,key);else print(key in revoked.proxy);}catch(error) {print(error.kind);}
                print(events.join(','));
                events=[];revoked=Proxy.revocable({},handler);
                key={toString() {events.push('key');revoked.revoke();$262.gc();return 'entry';}};
                try {if (remove) Reflect.deleteProperty(revoked.proxy,key);else print(key in revoked.proxy);}catch(error) {print(error instanceof TypeError);}
                print(events.join(','));
            }
            "#,
            &[
                "true",
                "key,lookup,trap:entry",
                "coercion",
                "key",
                "true",
                "key",
                "true",
                "key,lookup,trap:entry",
                "coercion",
                "key",
                "true",
                "key",
            ],
        );
    }

    #[test]
    fn regression_proxy_presence_invariants_skip_extensibility_for_frozen_properties() {
        assert_output_in_execution_modes(
            r#"
            for (var remove of [false,true]) {
                var events=[],raw={};Object.defineProperty(raw,'entry',{value:42,configurable:false});
                var target=new Proxy(raw,{getOwnPropertyDescriptor(object,key) {events.push('descriptor');$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);},isExtensible() {events.push('extensible');throw {kind:'extensible'};}});
                var source=new Proxy(target,{has() {events.push('has');return false;},deleteProperty() {events.push('delete');return true;}});
                try {if (remove) Reflect.deleteProperty(source,'entry');else Reflect.has(source,'entry');}catch(error) {print(error instanceof TypeError);}
                print(events.join(','));
                events=[];raw={entry:42};target=new Proxy(raw,{getOwnPropertyDescriptor(object,key) {events.push('descriptor');$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);},isExtensible(object) {events.push('extensible');$262.gc();return Reflect.isExtensible(object);}});
                source=new Proxy(target,{has() {events.push('has');return false;},deleteProperty() {events.push('delete');return true;}});
                print(remove?Reflect.deleteProperty(source,'entry'):Reflect.has(source,'entry'));print(events.join(','));
                Object.preventExtensions(raw);events=[];
                try {if (remove) Reflect.deleteProperty(source,'entry');else Reflect.has(source,'entry');}catch(error) {print(error instanceof TypeError);}
                print(events.join(','));
            }
            "#,
            &[
                "true",
                "has,descriptor",
                "false",
                "has,descriptor,extensible",
                "true",
                "has,descriptor,extensible",
                "true",
                "delete,descriptor",
                "true",
                "delete,descriptor,extensible",
                "true",
                "delete,descriptor,extensible",
            ],
        );
    }

    #[test]
    fn regression_proxy_invocations_keep_captured_operands_after_revocation() {
        assert_output_in_execution_modes(
            r#"
            for (var construct of [false,true]) {
                for (var fallback of [false,true]) {
                    var events=[],arg={rank:47},receiver={rank:46};
                    function Target(value) {$262.gc();events.push('target');return {rank:42,arg:value,receiver:this};}
                    function Destination() {}
                    var handler={};
                    Object.defineProperty(handler,construct?'construct':'apply',{get() {
                        events.push('lookup');revocable.revoke();$262.gc();
                        if (fallback) return null;
                        return function(target,second,third) {
                            $262.gc();events.push('trap');
                            print(this===handler);print(target===Target);
                            print(Array.isArray(construct?second:third));
                            print((construct?third:second)===(construct?Destination:receiver));
                            return {rank:42,arg:(construct?second:third)[0]};
                        };
                    }});
                    var revocable=Proxy.revocable(Target,handler);
                    var result=construct?Reflect.construct(revocable.proxy,[arg],Destination):Reflect.apply(revocable.proxy,receiver,[arg]);
                    print(result.rank);print(result.arg===arg);print(result.arg.rank);print(events.join(','));
                    try {Reflect.apply(revocable.proxy,receiver,[]);}catch(error) {print(error instanceof TypeError);}
                }
            }
            "#,
            &[
                "true",
                "true",
                "true",
                "true",
                "42",
                "true",
                "47",
                "lookup,trap",
                "true",
                "42",
                "true",
                "47",
                "lookup,target",
                "true",
                "true",
                "true",
                "true",
                "true",
                "42",
                "true",
                "47",
                "lookup,trap",
                "true",
                "42",
                "true",
                "47",
                "lookup,target",
                "true",
            ],
        );
    }

    #[test]
    fn regression_proxy_mutation_fallback_keeps_captured_targets_alive() {
        assert_output_in_execution_modes(
            r#"
            var revocable=Proxy.revocable({}, {get setPrototypeOf() {revocable.revoke();$262.gc();return null;}});
            print(Reflect.setPrototypeOf(revocable.proxy,{rank:42}));
            revocable=Proxy.revocable({}, {get preventExtensions() {revocable.revoke();$262.gc();return undefined;}});
            print(Reflect.preventExtensions(revocable.proxy));
            try {Object.preventExtensions(revocable.proxy);}catch(error) {print(error instanceof TypeError);}
            revocable=Proxy.revocable({},{});revocable.revoke();
            var object={};print(Object.setPrototypeOf(object,revocable.proxy)===object);print(Object.getPrototypeOf(object)===revocable.proxy);
            print(Object.setPrototypeOf(1,null));print(Object.preventExtensions(null));
            "#,
            &["true", "true", "true", "true", "true", "1", "null"],
        );
    }

    #[test]
    fn regression_object_and_reflect_share_boolean_mutation_semantics() {
        assert_output_in_execution_modes(
            r#"
            var target={},prototype={rank:43},events=[];
            var source=new Proxy(target,{setPrototypeOf(object,proto) {events.push('set');$262.gc();return false;},preventExtensions() {events.push('prevent');$262.gc();return false;}});
            print(Reflect.setPrototypeOf(source,prototype));try {Object.setPrototypeOf(source,prototype);}catch(error) {print(error instanceof TypeError);}
            print(Reflect.preventExtensions(source));try {Object.preventExtensions(source);}catch(error) {print(error instanceof TypeError);}
            print(events.join(','));
            Object.preventExtensions(target);print(Reflect.setPrototypeOf(target,Object.getPrototypeOf(target)));print(Reflect.setPrototypeOf(target,prototype));
            try {Object.setPrototypeOf(target,prototype);}catch(error) {print(error instanceof TypeError);}
            print(Object.setPrototypeOf(Object.prototype,null)===Object.prototype);print(Reflect.setPrototypeOf(Object.prototype,prototype));
            var plain={},other=Object.create(plain);print(Reflect.setPrototypeOf(plain,other));
            var reads=0;var exotic=new Proxy({}, {getPrototypeOf() {reads++;throw new Error('cycle walk');}});
            print(Reflect.setPrototypeOf(plain,exotic));print(Object.getPrototypeOf(plain)===exotic);print(reads);
            "#,
            &[
                "false",
                "true",
                "false",
                "true",
                "set,set,prevent,prevent",
                "true",
                "false",
                "true",
                "true",
                "false",
                "false",
                "true",
                "true",
                "0",
            ],
        );
    }

    #[test]
    fn regression_proxy_reads_keep_fresh_results_through_descriptor_validation() {
        assert_output_in_execution_modes(
            r#"
            var symbol=Symbol('entry'),events=[];
            var target=new Proxy({}, {getOwnPropertyDescriptor(object,key) {events.push('descriptor');$262.gc();return undefined;}});
            var source=new Proxy(target, {get get() {events.push('lookup');$262.gc();return function(object,key,receiver) {
                events.push('trap');return {rank:42,key:key,receiver:receiver};
            };}});
            print(source.entry.rank);print(events.join(','));events=[];
            var receiver={rank:43};var result=Reflect.get(source,symbol,receiver);
            print(result.rank);print(result.key===symbol);print(result.receiver===receiver);print(events.join(','));
            var revocable=Proxy.revocable({get entry() {$262.gc();return this.rank;}}, {get get() {revocable.revoke();$262.gc();return null;}});
            print(Reflect.get(revocable.proxy,'entry',{rank:44}));
            try {revocable.proxy.entry;}catch(error) {print(error instanceof TypeError);}
            revocable=Proxy.revocable({[symbol]:45},{get get() {revocable.revoke();$262.gc();return undefined;}});
            print(revocable.proxy[symbol]);
            try {revocable.proxy[symbol];}catch(error) {print(error instanceof TypeError);}
            "#,
            &[
                "42",
                "lookup,trap,descriptor",
                "42",
                "true",
                "true",
                "lookup,trap,descriptor",
                "44",
                "true",
                "45",
                "true",
            ],
        );
    }

    #[test]
    fn regression_proxy_prototype_results_survive_validation_and_revocation() {
        assert_output_in_execution_modes(
            r#"
            var target=new Proxy({}, {isExtensible(object) {$262.gc();return Reflect.isExtensible(object);}});
            var source=new Proxy(target, {getPrototypeOf() {return {rank:42};}});
            print(Object.getPrototypeOf(source).rank);print(Reflect.getPrototypeOf(source).rank);
            var revocable=Proxy.revocable(Object.create({rank:43}), {get getPrototypeOf() {revocable.revoke();$262.gc();return null;}});
            print(Object.getPrototypeOf(revocable.proxy).rank);
            try {Object.getPrototypeOf(revocable.proxy);}catch(error) {print(error instanceof TypeError);}
            print(Object.getPrototypeOf('ab')===String.prototype);
            "#,
            &["42", "42", "43", "true", "true"],
        );
    }

    #[test]
    fn regression_proxy_descriptor_coercion_and_validation_order() {
        assert_output_in_execution_modes(
            r#"
            var events=[];
            var target=new Proxy({}, {
                getOwnPropertyDescriptor() {events.push('target-desc');$262.gc();return undefined;},
                isExtensible() {events.push('extensible');$262.gc();return true;}
            });
            var source=new Proxy(target, {get getOwnPropertyDescriptor() {events.push('get-trap');$262.gc();return function() {
                events.push('trap');return {get enumerable() {events.push('enumerable');$262.gc();return true;},configurable:true,
                    get value() {events.push('value');$262.gc();return {rank:44};},writable:true};
            };}});
            var key={toString() {events.push('key');$262.gc();return 'entry';}};
            var result=Object.getOwnPropertyDescriptor(source,key);print(result.value.rank);print(events.join(','));
            events=[];result=Reflect.getOwnPropertyDescriptor(source,key);print(result.value.rank);print(events.join(','));
            events=[];
            source=new Proxy(target,{getOwnPropertyDescriptor() {events.push('trap');return undefined;}});
            print(Object.getOwnPropertyDescriptor(source,'missing'));print(events.join(','));
            "#,
            &[
                "44",
                "key,get-trap,trap,target-desc,extensible,enumerable,value",
                "44",
                "key,get-trap,trap,target-desc,extensible,enumerable,value",
                "undefined",
                "trap,target-desc",
            ],
        );
    }

    #[test]
    fn regression_proxy_descriptor_compatibility_uses_shared_records() {
        assert_output_in_execution_modes(
            r#"
            var target={};Object.defineProperty(target,'entry',{value:1,writable:true,configurable:false});
            var descriptor={value:2,writable:true,configurable:false};
            var source=new Proxy(target,{getOwnPropertyDescriptor() {return descriptor;}});
            print(Object.getOwnPropertyDescriptor(source,'entry').value);
            descriptor.writable=false;try {Object.getOwnPropertyDescriptor(source,'entry');}catch(error) {print(error instanceof TypeError);}
            descriptor={get:undefined,configurable:false};try {Object.getOwnPropertyDescriptor(source,'entry');}catch(error) {print(error instanceof TypeError);}
            var revocable=Proxy.revocable({entry:45},{get getOwnPropertyDescriptor() {revocable.revoke();$262.gc();return undefined;}});
            print(Object.getOwnPropertyDescriptor(revocable.proxy,'entry').value);
            try {Object.getOwnPropertyDescriptor(revocable.proxy,'entry');}catch(error) {print(error instanceof TypeError);}
            var badKey={toString() {throw {kind:'key'};}};
            try {Object.getOwnPropertyDescriptor(null,badKey);}catch(error) {print(error instanceof TypeError);}
            "#,
            &["2", "true", "true", "45", "true", "true"],
        );
    }

    #[test]
    fn regression_for_in_preserves_collecting_key_snapshots() {
        assert_output_in_execution_modes(
            r#"
            function source() {return new Proxy({first:1,later:2}, {
                getOwnPropertyDescriptor(object,key) {$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);},
                getPrototypeOf() {$262.gc();return new Proxy({inherited:3}, {
                    getOwnPropertyDescriptor(object,key) {$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);},
                    getPrototypeOf() {$262.gc();return null;}
                });}
            });}
            var keys=[];for(var key in source()) {$262.gc();keys.push(key);}print(keys.join(','));
            var object=Object.create({hidden:1,inherited:2});
            Object.defineProperty(object,'hidden',{value:3,enumerable:false});object.first=4;
            keys=[];for(var key in object) {keys.push(key);}print(keys.join(','));
            keys=[];for(var key in 'ab') {keys.push(key);}print(keys.join(','));
            keys=[];for(var key in null) {keys.push(key);}for(var key in undefined) {keys.push(key);}print(keys.length);
            "#,
            &["first,later,inherited", "first,inherited", "0,1", "0"],
        );
    }

    #[test]
    fn regression_property_copy_snapshots_survive_collecting_getters() {
        assert_output_in_execution_modes(
            r#"
            var symbol=Symbol('entry');
            function source() {return new Proxy({first:42, later:43, [symbol]:44}, {
                getOwnPropertyDescriptor(object,key) {$262.gc(); return Reflect.getOwnPropertyDescriptor(object,key);},
                get(object,key) {$262.gc(); return Reflect.get(object,key);}
            });}
            var target=Object.assign(1,source());
            print(target.valueOf()); print(target.first);print(target.later);print(target[symbol]);
            var spread={...source()};print(spread.first);print(spread.later);print(spread[symbol]);
            var {first,...rest}=source(); print(first);print(Object.hasOwn(rest,'first'));print(rest.later);print(rest[symbol]);
            var getterReads=0;
            var overwritten={get first() {getterReads++;return 99;},...{first:42}};
            print(overwritten.first);print(Object.getOwnPropertyDescriptor(overwritten,'first').writable);print(getterReads);
            var setterCalls=0;
            Object.assign({set first(value) {$262.gc();setterCalls++;print(value.rank);}}, {get first() {$262.gc();return {rank:45};}});
            print(setterCalls);
            Object.assign=function() {throw new Error('overridden assign');};
            print(({...{first:46}}).first);print(Object.keys({...null,...undefined}).length);
            "#,
            &[
                "1", "42", "43", "44", "42", "43", "44", "42", "false", "43", "44", "42", "true", "0",
                "45", "1", "46", "0",
            ],
        );
    }

    #[test]
    fn regression_descriptor_snapshot_preserves_fresh_destination() {
        assert_output_in_execution_modes(
            r#"
            var symbol=Symbol('entry'), events=[];
            var source=new Proxy({first:42,later:43,[symbol]:44}, {
                getOwnPropertyDescriptor(object,key) {events.push(typeof key==='symbol' ? 'symbol' : key);$262.gc();return Reflect.getOwnPropertyDescriptor(object,key);}
            });
            var result=Object.getOwnPropertyDescriptors(source);
            print(result.first.value); print(result.later.value); print(result[symbol].value);print(events.join(','));
            print(Object.keys(result).join(','));
            var attributes=Object.getOwnPropertyDescriptor(result,'first');print(attributes.writable && attributes.enumerable && attributes.configurable);
            print(Object.getOwnPropertyDescriptors('ab')[0].value);
            "#,
            &[
                "42",
                "43",
                "44",
                "first,later,symbol",
                "first,later",
                "true",
                "a",
            ],
        );
    }

    #[test]
    fn regression_array_like_arguments_keep_earlier_collecting_values() {
        assert_output_in_execution_modes(
            r#"
            function list() {return {length:{valueOf() {$262.gc(); return 2;}},
                get 0() {return {rank:42};}, get 1() {$262.gc(); return {rank:43};}};}
            function called(first, second) {print(first.rank); print(second.rank);}
            Reflect.apply(called, null, list()); called.apply(null, list());
            var result = Reflect.construct(function(first,second) {this.first=first; this.second=second;}, list());
            print(result.first.rank); print(result.second.rank);
            var events=[];
            var proxy = new Proxy({}, {ownKeys() {return {length:2,
                get 0() {events.push('first'); return 'fresh-' + events.length;},
                get 1() {$262.gc(); events.push('second'); return Symbol('later');}};}});
            var keys=Reflect.ownKeys(proxy);
            print(keys[0]); print(keys[1].description); print(events.join(','));
            "#,
            &[
                "42",
                "43",
                "42",
                "43",
                "42",
                "43",
                "fresh-1",
                "later",
                "first,second",
            ],
        );
    }

    #[test]
    fn regression_proxy_own_keys_checks_types_and_observable_target_invariants() {
        assert_output_in_execution_modes(
            r#"
            function outcome(action) {try {return String(action());} catch(error) {return error instanceof TypeError ? 'TypeError' : error;}}
            var events=[];
            print(outcome(function() {return Reflect.ownKeys(new Proxy({}, {ownKeys() {return {
                length:2, get 0() {events.push('invalid'); return undefined;},
                get 1() {events.push('later'); throw 'later error';}
            };}}));})); print(events.join(','));
            for (var sealed of [false,true]) {
                var events=[], target={first:42, later:43};
                Object.defineProperty(target,'first',{configurable:false});
                if (sealed) Object.preventExtensions(target);
                var inner=new Proxy(target, {isExtensible(object) {events.push('extensible'); $262.gc(); return Reflect.isExtensible(object);},
                    ownKeys(object) {events.push('target-keys'); $262.gc(); return Reflect.ownKeys(object);},
                    getOwnPropertyDescriptor(object,key) {events.push('descriptor:' + key); $262.gc(); return Reflect.getOwnPropertyDescriptor(object,key);}});
                var outer=new Proxy(inner,{ownKeys() {events.push('trap'); return ['later','first'];}});
                print(Reflect.ownKeys(outer).join(',')); print(events.join(','));
                events=[];
                print(outcome(function() {return Reflect.ownKeys(new Proxy(inner,{ownKeys() {events.push('trap'); return ['later'];}}));}));
                print(events.join(','));
            }
            "#,
            &[
                "TypeError",
                "invalid",
                "later,first",
                "trap,extensible,target-keys,descriptor:first,descriptor:later",
                "TypeError",
                "trap,extensible,target-keys,descriptor:first,descriptor:later",
                "later,first",
                "trap,extensible,target-keys,descriptor:first,descriptor:later",
                "TypeError",
                "trap,extensible,target-keys,descriptor:first,descriptor:later",
            ],
        );
    }

    #[test]
    fn regression_own_enumeration_survives_collecting_callbacks() {
        assert_output_in_execution_modes(
            r#"
            for (var kind of ['values', 'entries']) {
                var events = [];
                var object = {get first() {events.push('first'); return {rank:42};},
                    get second() {$262.gc(); events.push('second'); return 43;}};
                var result = Object[kind](object);
                print(kind === 'values' ? result[0].rank : result[0][1].rank);
                print(kind === 'values' ? result[1] : result[1][1]);
                print(events.join(','));
            }
            for (var kind of ['keys', 'values', 'entries']) {
                var events = [], reads = 0;
                var object = new Proxy({}, {
                    ownKeys() {return ['first','later',Symbol('ignored')];},
                    getOwnPropertyDescriptor(object,key) {
                        events.push('descriptor:' + key); $262.gc();
                        return {value:undefined, enumerable:true, configurable:true, writable:true};
                    },
                    get(object,key) {reads++; events.push('get:' + key); $262.gc(); return {rank:42};}
                });
                var result = Object[kind](object);
                print(result.length);
                print(kind === 'keys' ? result.join(',') : kind === 'values' ? result[0].rank + ',' + result[1].rank : result[0][0] + ':' + result[0][1].rank + ',' + result[1][0] + ':' + result[1][1].rank);
                print(reads); print(events.join(','));
            }
            "#,
            &[
                "42",
                "43",
                "first,second",
                "42",
                "43",
                "first,second",
                "2",
                "first,later",
                "0",
                "descriptor:first,descriptor:later",
                "2",
                "42,42",
                "2",
                "descriptor:first,get:first,descriptor:later,get:later",
                "2",
                "first:42,later:42",
                "2",
                "descriptor:first,get:first,descriptor:later,get:later",
            ],
        );
    }

    #[test]
    fn regression_object_create_preserves_collected_descriptor_records() {
        assert_output_in_execution_modes(
            r#"
            var reads = 0;
            var result = Object.create(null, {
                first: {get value() {reads++; return {rank:42};}},
                second: {get get() {reads++; $262.gc(); return function() {return 43;};}},
                third: {get value() {$262.gc(); return 44;}}
            });
            print(Object.getPrototypeOf(result) === null); print(result.first.rank);
            print(result.second); print(result.third); print(reads);
            var target = {};
            var marker = {rank:45};
            try {Object.defineProperties(target, {first:{value:42}, get second() {$262.gc(); throw marker;}});}
            catch (error) {print(error === marker);}
            print(Object.hasOwn(target,'first'));
            "#,
            &["true", "42", "43", "44", "2", "true", "false"],
        );
    }

    #[test]
    fn regression_property_definition_parses_once_before_proxy_effects() {
        assert_output_in_execution_modes(
            r#"
            for (var define of [Object.defineProperty, Reflect.defineProperty]) {
                var events = [];
                var input = {enumerable:true, configurable:true,
                    get value() {events.push('value'); $262.gc(); return {rank:42};},
                    get writable() {events.push('writable'); $262.gc(); return true;}};
                var target = {};
                var proxy = new Proxy(target, {get defineProperty() {events.push('trap-getter'); $262.gc();
                    return function(target, key, descriptor) {
                        events.push('trap'); $262.gc(); print(descriptor === input);
                        print(Reflect.ownKeys(descriptor).join(','));
                        Object.defineProperty(target, key, descriptor); return true;
                    };
                }});
                define(proxy, {[Symbol.toPrimitive]() {events.push('key'); $262.gc(); return 'answer';}}, input);
                print(events.join(','));
                print(target.answer.rank);
            }
            "#,
            &[
                "false",
                "value,writable,enumerable,configurable",
                "key,value,writable,trap-getter,trap",
                "42",
                "false",
                "value,writable,enumerable,configurable",
                "key,value,writable,trap-getter,trap",
                "42",
            ],
        );
    }

    #[test]
    fn regression_reflect_definition_preserves_thrown_completion() {
        assert_output_in_execution_modes(
            r#"
            var marker = {rank:42};
            try {Reflect.defineProperty([], 'length', {value:{valueOf() {$262.gc(); throw marker;}}}); print(false);}
            catch (error) {print(error === marker);}
            try {Reflect.defineProperty([], 'length', {value:-1}); print(false);}
            catch (error) {print(error instanceof RangeError);}
            var frozen = Object.freeze({answer:1});
            print(Reflect.defineProperty(frozen, 'answer', {value:2}));
            print(Reflect.defineProperty(frozen, 'other', {value:2}));
            print(Reflect.defineProperty(new Uint8Array(0), '0', {value:2}));
            "#,
            &["true", "true", "false", "false", "false"],
        );
    }

    #[test]
    fn regression_internal_definitions_ignore_descriptor_prototype_fields() {
        assert_output_in_execution_modes(
            r#"
            Object.defineProperty(Object.prototype, 'get', {configurable:true, get() {$262.gc(); throw 'inherited-get';}});
            var reflected, mapped, flattened, from;
            try {
                var descriptor = Object.create(null); descriptor.value = {rank:42}; descriptor.configurable = true;
                reflected = {}; print(Reflect.defineProperty(reflected, 'answer', descriptor));
                mapped = [1,2].map(value => value + 1);
                flattened = [[1],[2]].flat();
                from = Array.from([1,2], value => value + 2);
            } finally {delete Object.prototype.get;}
            print(reflected.answer.rank); print(mapped.join(',')); print(flattened.join(',')); print(from.join(','));
            "#,
            &["true", "42", "2,3", "1,2", "3,4"],
        );
    }

    #[test]
    fn regression_proxy_definition_invariants_use_original_record() {
        assert_output_in_execution_modes(
            r#"
            for (var define of [Object.defineProperty, Reflect.defineProperty]) {
                var proxy = new Proxy({}, {defineProperty(target,key,descriptor) {
                    descriptor.configurable = true; $262.gc(); return true;
                }});
                try {define(proxy, 'answer', {value:42, configurable:false}); print(false);}
                catch (error) {print(error instanceof TypeError);}
            }
            "#,
            &["true", "true"],
        );
    }

    #[test]
    fn regression_constructor_operands_survive_collecting_prototype_lookup() {
        assert_output_in_execution_modes(
            r#"
            var result = new (new Proxy(function C(value) {
                $262.gc(); this.answer = value.answer;
            }, {get(target, key, receiver) {
                if (key === 'prototype') {$262.gc(); return {};}
                return Reflect.get(target, key, receiver);
            }}))({answer: 42});
            print(result.answer);
            "#,
            &["42"],
        );
    }

    #[test]
    fn regression_collecting_getters_keep_current_instruction_registers() {
        assert_output_in_execution_modes(
            r#"
            var source = {get first() {return {answer: 42};}, get second() {$262.gc(); return 7;}};
            print([source.first, source.second][0].answer);
            print(({first: source.first, second: source.second}).first.answer);
            function first(left, right) {return left.answer;}
            print(first(source.first, source.second));
            "#,
            &["42", "42", "42"],
        );
    }

    #[test]
    fn regression_spread_and_aggregate_lists_survive_collection() {
        assert_output_in_execution_modes(
            r#"
            for (var mode of ['spread', 'aggregate']) {
                var nextReads = 0;
                var source = {[Symbol.iterator]() {
                    var index = 0;
                    return {get next() {
                        nextReads++;
                        $262.gc();
                        return function() {
                            $262.gc();
                            if (index === 3) return {done: true};
                            return {get done() {$262.gc(); return false;}, value: {rank: ++index}};
                        };
                    }};
                }};
                var result = mode === 'spread' ? [...source] : new AggregateError(source).errors;
                print(result.length);
                print(result.map(value => value.rank).join(','));
                print(nextReads);
            }
            var cause = {marker: 7};
            var error = new AggregateError([{rank: 1}], {toString() {$262.gc(); return 'message';}}, {
                get cause() {$262.gc(); return cause;}
            });
            print(error.message);
            print(error.cause === cause);
            print(error.errors[0].rank);
            print(Object.getOwnPropertyDescriptor(error, 'errors').enumerable);
            "#,
            &[
                "3", "1,2,3", "1", "3", "1,2,3", "1", "message", "true", "1", "false",
            ],
        );
    }

    #[test]
    fn regression_aggregate_error_preserves_effect_order() {
        assert_output_in_execution_modes(
            r#"
            var log = [];
            var prototype = {};
            var target = new Proxy(function Other() {}, {get(object, key, receiver) {
                if (key === 'prototype') {log.push('prototype'); $262.gc(); return prototype;}
                return Reflect.get(object, key, receiver);
            }});
            var cause = {marker: 7};
            var options = new Proxy({}, {
                has(object, key) {log.push('has'); $262.gc(); return true;},
                get(object, key) {log.push('cause'); $262.gc(); return cause;}
            });
            var source = {get [Symbol.iterator]() {log.push('iterator'); $262.gc(); return function() {
                var index = 0;
                return {next() {log.push('next'); $262.gc(); return index++ ? {done: true} : {value: {rank: 1}, done: false};}};
            };}};
            var message = {toString() {log.push('message'); $262.gc(); return 'message';}};
            var error = Reflect.construct(AggregateError, [source, message, options], target);
            print(log.join(','));
            print(Object.getPrototypeOf(error) === prototype);
            print(error.cause === cause);
            print(error.errors[0].rank);
            for (var key of ['message', 'cause', 'errors']) {
                var descriptor = Object.getOwnPropertyDescriptor(error, key);
                print(descriptor.writable && descriptor.configurable && !descriptor.enumerable);
            }
            var SavedAggregateError = AggregateError;
            AggregateError = function Poison() {};
            function InvalidPrototypeTarget() {}
            InvalidPrototypeTarget.prototype = null;
            var fallback = Reflect.construct(SavedAggregateError, [[]], InvalidPrototypeTarget);
            print(Object.getPrototypeOf(fallback) === SavedAggregateError.prototype);
            AggregateError = SavedAggregateError;
            for (var mode of ['spread', 'aggregate']) {
                for (var phase of ['next', 'done', 'value']) {
                    var marker = {};
                    var closed = 0;
                    var source = {[Symbol.iterator]() {return {
                        next() {
                            if (phase === 'next') throw marker;
                            return {get done() {if (phase === 'done') throw marker; return false;}, get value() {throw marker;}};
                        },
                        return() {closed++; return {};}
                    };}};
                    try {if (mode === 'spread') [...source]; else new AggregateError(source);}
                    catch (error) {print(error === marker);}
                    print(closed);
                }
            }
            "#,
            &[
                "prototype,message,has,cause,iterator,next,next",
                "true",
                "true",
                "1",
                "true",
                "true",
                "true",
                "true",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
                "true",
                "0",
            ],
        );
    }

    #[test]
    fn regression_typed_iterable_roots_survive_collection() {
        assert_output_in_execution_modes(
            r#"
            for (var mode of ['construct', 'from']) {
                for (var Constructor of [Uint8Array, BigInt64Array, BigUint64Array]) {
                    var bigint = Constructor !== Uint8Array;
                    var source = {[Symbol.iterator]() {
                        var index = 0;
                        return {next() {
                            $262.gc();
                            if (index === 3) return {done: true};
                            var rank = ++index;
                            return {get done() {$262.gc(); return false;}, get value() {
                                return {valueOf() {$262.gc(); return bigint ? BigInt(rank) : rank;}};
                            }};
                        }};
                    }};
                    var result = mode === 'construct' ? new Constructor(source) : Constructor.from(source);
                    print(result.join(','));
                }
            }
            for (var mode of ['construct', 'from']) {
                var log = [];
                var marker = {};
                var source = {[Symbol.iterator]() {return {
                    next() {throw marker;},
                    return() {log.push('close'); return {};}
                };}};
                try {if (mode === 'construct') new Uint8Array(source); else Uint8Array.from(source);}
                catch (error) {print(error === marker);}
                print(log.length);
            }
            "#,
            &[
                "1,2,3", "1,2,3", "1,2,3", "1,2,3", "1,2,3", "1,2,3", "true", "0", "true", "0",
            ],
        );
    }

    #[test]
    fn regression_typed_array_like_conversion_is_incremental() {
        assert_output_in_execution_modes(
            r#"
            for (var Constructor of [Uint8Array, BigInt64Array, BigUint64Array]) {
                var log = [];
                var converted = false;
                var source = {
                    get [Symbol.iterator]() {$262.gc(); return undefined;},
                    get length() {$262.gc(); return 2;},
                    get 0() {log.push('get0'); return {valueOf() {$262.gc(); log.push('convert0'); converted = true; return '7';}};},
                    get 1() {log.push('get1'); return {valueOf() {$262.gc(); log.push('convert1'); return converted ? '8' : '0';}};}
                };
                print(new Constructor(source).join(','));
                print(log.join(','));
                print(new Constructor({length: -1}).length);
                try {new Constructor({length: Number.MAX_SAFE_INTEGER + 1});}
                catch (error) {print(error instanceof RangeError);}
            }
            "#,
            &[
                "7,8",
                "get0,convert0,get1,convert1",
                "0", "true",
                "7,8",
                "get0,convert0,get1,convert1",
                "0", "true",
                "7,8",
                "get0,convert0,get1,convert1",
                "0", "true",
            ],
        );
    }

    #[test]
    fn regression_typed_fill_value_survives_collecting_bounds() {
        assert_output_in_execution_modes(
            r#"
            for (var Constructor of [BigInt64Array, BigUint64Array]) {
                for (var bound of ['start', 'end']) {
                    var log = [];
                    var source = new Constructor([1n, 2n, 3n]);
                    var value = {valueOf() {log.push('value'); return 7n;}};
                    var index = {valueOf() {log.push(bound); $262.gc(); return bound === 'start' ? 1 : 2;}};
                    var result = bound === 'start' ? source.fill(value, index) : source.fill(value, 0, index);
                    print(source.join(','));
                    print(result === source);
                    print(log.join(','));
                }
            }
            for (var phase of ['value', 'start', 'end']) {
                var source = new BigInt64Array(3);
                var log = [];
                function step(name, result) {
                    return {valueOf() {
                        log.push(name);
                        if (phase === name) { $262.detachArrayBuffer(source.buffer); $262.gc(); }
                        return result;
                    }};
                }
                try { source.fill(step('value', 7n), step('start', 0), step('end', 2)); }
                catch (error) {print(error instanceof TypeError);}
                print(log.join(','));
            }
            "#,
            &[
                "1,7,7",
                "true",
                "value,start",
                "7,7,3",
                "true",
                "value,end",
                "1,7,7",
                "true",
                "value,start",
                "7,7,3",
                "true",
                "value,end",
                "true",
                "value,start,end",
                "true",
                "value,start,end",
                "true",
                "value,start,end",
            ],
        );
    }

    #[test]
    fn regression_typed_sort_snapshots_survive_collecting_comparators() {
        assert_output_in_execution_modes(
            r#"
            for (var Constructor of [BigInt64Array, BigUint64Array]) {
                for (var method of ['sort', 'toSorted']) {
                    var source = new Constructor([3n, 1n, 2n, 4n]);
                    var result = source[method]((left, right) => {
                        $262.gc();
                        return {valueOf() {
                            $262.gc();
                            return left < right ? -1 : left > right ? 1 : 0;
                        }};
                    });
                    print(result.join(','));
                    print(result === source);
                }
            }
            var numeric = new Float64Array([NaN, 0, -0, 2, -1]).toSorted();
            print(numeric[0]);
            print(Object.is(numeric[1], -0));
            print(Object.is(numeric[2], 0));
            print(numeric[3]);
            print(Number.isNaN(numeric[4]));
            "#,
            &[
                "1,2,3,4", "true", "1,2,3,4", "false", "1,2,3,4", "true", "1,2,3,4", "false", "-1",
                "true", "true", "2", "true",
            ],
        );
    }

    #[test]
    fn regression_array_sort_snapshots_survive_collecting_guest_effects() {
        assert_output_in_execution_modes(
            r#"
            for (var method of ['sort', 'toSorted']) {
                for (var custom of [true, false]) {
                    var source = [0, 1, 2];
                    var written = [];
                    for (let index of [0, 1, 2]) {
                        Object.defineProperty(source, index, {
                            get() {
                                if (written[index]) return written[index];
                                $262.gc();
                                return {rank: 3 - index, toString() {$262.gc(); return String(this.rank);}};
                            },
                            set(value) {$262.gc(); written[index] = value;}
                        });
                    }
                    var result = custom ? source[method]((left, right) => {
                        $262.gc(); return {valueOf() {$262.gc(); return left.rank - right.rank;}};
                    }) : source[method]();
                    print(result.map(value => value.rank).join(','));
                    print(result === source);
                }
            }
            "#,
            &["1,2,3", "true", "1,2,3", "true", "1,2,3", "false", "1,2,3", "false"],
        );
    }

    #[test]
    fn regression_array_copies_root_values_across_collecting_getters() {
        assert_output_in_execution_modes(
            r#"
            for (var method of ['with', 'toReversed', 'toSpliced']) {
                var trace = [];
                var source = [0, 1, 2];
                for (let index of [0, 1, 2]) {
                    Object.defineProperty(source, index, {get() {
                        trace.push(index);
                        $262.gc();
                        return {answer: 40 + index};
                    }});
                }
                var replacement = {answer: 99};
                var result = method === 'with' ? source.with(1, replacement)
                    : method === 'toSpliced' ? source.toSpliced(1, 1, replacement) : source.toReversed();
                print(result.map(value => value.answer).join(','));
                print(trace.join(','));
                print(Object.getPrototypeOf(result) === Array.prototype);
            }
            print(Array.prototype.with.call('abc', {valueOf() {$262.gc(); return 1;}}, 'z').join(','));
            print(Array.prototype.toSpliced.call('abc', {valueOf() {$262.gc(); return 1;}}, 1, 'z').join(','));
            "#,
            &[
                "40,99,42", "0,2", "true",
                "42,41,40", "2,1,0", "true",
                "40,99,42", "0,2", "true", "a,z,c", "a,z,c",
            ],
        );
    }

    #[test]
    fn regression_reduction_accumulators_survive_guest_collection() {
        assert_output_in_execution_modes(
            r#"
            for (var method of ['reduce', 'reduceRight']) {
                var source = [0, 1];
                Object.defineProperty(source, method === 'reduce' ? '1' : '0', {
                    get() { $262.gc(); return 1; }
                });
                print(source[method]((accumulator, value) => ({sum: accumulator.sum + 1}), {sum: 40}).sum);
                var seed = [0, 1];
                Object.defineProperty(seed, method === 'reduce' ? '0' : '1', {
                    get() { return {sum: 40}; }
                });
                Object.defineProperty(seed, method === 'reduce' ? '1' : '0', {
                    get() { $262.gc(); return {sum: 2}; }
                });
                print(seed[method]((accumulator, value) => ({sum: accumulator.sum + value.sum})).sum);
                var proxy = new Proxy([0, 1], {
                    has(target, key) { $262.gc(); return Reflect.has(target, key); }
                });
                print(proxy[method]((accumulator, value) => ({sum: accumulator.sum + 1}), {sum: 40}).sum);
                print(new Uint8Array([0, 1])[method]((accumulator, value) => {
                    $262.gc();
                    return {sum: accumulator.sum + 1};
                }, {sum: 40}).sum);
            }
            "#,
            &["42", "42", "42", "42", "42", "42", "42", "42"],
        );
    }

    #[test]
    fn regression_flattened_values_survive_collection() {
        assert_output_in_execution_modes(
            r#"
            var source = [0, 1];
            Object.defineProperty(source, '0', {get() { return [{answer: 42}]; }});
            Object.defineProperty(source, '1', {get() { $262.gc(); return [{answer: 43}]; }});
            print(source.flat().map(value => value.answer).join(','));
            print([0, 1].flatMap(value => {
                $262.gc();
                return [{answer: 44 + value}];
            }).map(value => value.answer).join(','));
            "#,
            &["42,43", "44,45"],
        );
    }

    #[test]
    fn regression_flattening_species_writes_follow_each_element() {
        assert_output_in_execution_modes(
            r#"
            for (var method of ['flat', 'flatMap']) {
                var trace = [];
                var source = [1, 2];
                source.constructor = {[Symbol.species]: function() {
                    return new Proxy({}, {defineProperty(target, key, descriptor) {
                        trace.push('write' + key);
                        Object.defineProperty(target, key, descriptor);
                        return true;
                    }});
                }};
                Object.defineProperty(source, '0', {get() { trace.push('get0'); return 1; }});
                Object.defineProperty(source, '1', {get() { trace.push('get1'); return 2; }});
                var result = method === 'flat' ? source.flat() : source.flatMap(value => {
                    trace.push('map' + value);
                    return [value];
                });
                print(trace.join(','));
                print(result[0] + ',' + result[1]);
            }
            "#,
            &["get0,write0,get1,write1", "1,2", "get0,map1,write0,get1,map2,write1", "1,2"],
        );
    }

    #[test]
    fn regression_flattening_stops_when_species_write_fails() {
        assert_output_in_execution_modes(
            r#"
            var trace = [];
            var source = [1, 2];
            source.constructor = {[Symbol.species]: function() {
                return new Proxy({}, {defineProperty() {trace.push('write'); return false;}});
            }};
            try {
                source.flatMap(value => {trace.push('map' + value); return [value];});
            } catch (error) { print(error instanceof TypeError); }
            print(trace.join(','));
            "#,
            &["true", "map1,write"],
        );
    }

    #[test]
    fn regression_bound_function_survives_collecting_metadata_getters() {
        assert_output_in_execution_modes(
            r#"
            function target(first, second) { return this.base + first.value + second; }
            Object.defineProperty(target, 'length', {
                configurable: true,
                get() { print('length'); $262.gc(); return 2; }
            });
            Object.defineProperty(target, 'name', {
                configurable: true,
                get() { print('name'); $262.gc(); return 'target'; }
            });
            var bound = target.bind({base: 40}, {value: 1});
            $262.gc();
            print(bound.name);
            print(bound.length);
            print(bound(1));
            var name = Object.getOwnPropertyDescriptor(bound, 'name');
            var length = Object.getOwnPropertyDescriptor(bound, 'length');
            print(name.writable + ',' + name.enumerable + ',' + name.configurable);
            print(length.writable + ',' + length.enumerable + ',' + length.configurable);
            for (var property of ['length', 'name']) {
                var marker = {};
                var victim = function() {};
                Object.defineProperty(victim, property, {
                    get() { $262.gc(); throw marker; }
                });
                try { victim.bind(null); }
                catch (error) { print(error === marker); }
            }
            "#,
            &[
                "length", "name", "bound target", "1", "42",
                "false,false,true", "false,false,true", "true", "true",
            ],
        );
    }

    #[test]
    fn regression_closure_identity_survives_collection() {
        assert_output_in_execution_modes(
            r#"
            function factory(value) {
                return function self() {
                    print(self === arguments.callee);
                    $262.gc();
                    return value;
                };
            }
            var first = factory(42);
            var second = factory(43);
            $262.gc();
            print(first());
            print(second());
            $262.gc();
            print(first());
            "#,
            &["true", "42", "true", "43", "true", "42"],
        );
    }

    #[test]
    fn regression_direct_eval_private_expressions_use_the_class_environment() {
        assert_output_in_execution_modes(
            r#"
            var Box = class {
                #value = 42;
                read() {
                    print(eval('this.#value'));
                    print(eval("'use strict'; this.#value"));
                    print(eval('this.#value += 1'));
                    var read = eval('() => this.#value');
                    $262.gc();
                    print(read());
                    print(eval('#value in this'));
                    try { eval('this.#missing'); }
                    catch (error) { print(error instanceof SyntaxError); }
                    try { (0, eval)('this.#value'); }
                    catch (error) { print(error instanceof SyntaxError); }
                    try { eval('({}).#value'); }
                    catch (error) { print(error instanceof TypeError); }
                }
            };
            new Box().read();
            "#,
            &["42", "42", "43", "43", "true", "true", "true", "true"],
        );
    }

    #[test]
    fn regression_strict_eval_retains_method_and_private_syntax_context() {
        assert_output_in_execution_modes(
            r#"
            var Derived = class {
                #value = 43;
                field = eval('() => super.value');
                read() {
                    print(this.field());
                    print(eval('super.value'));
                    try { eval('function inner() { return super.value; }'); }
                    catch (error) { print(error instanceof SyntaxError); }
                    try { eval('this.#missing'); }
                    catch (error) { print(error instanceof SyntaxError); }
                    try { eval('(() => function eval() {})'); }
                    catch (error) { print(error instanceof SyntaxError); }
                }
            };
            Object.setPrototypeOf(Derived.prototype, { get value() { return 42; } });
            new Derived().read();
            "#,
            &["42", "42", "true", "true", "true"],
        );
    }

    #[test]
    fn regression_parser_exhaustion_is_catchable_across_dynamic_entry_points() {
        std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                assert_output_in_execution_modes(
                    r#"
                    var intrinsicRangeError = RangeError;
                    RangeError = function() { throw 'replaced'; };
                    var parserStressDepth = 20000;
                    var source = '('.repeat(parserStressDepth) + '1' + ')'.repeat(parserStressDepth);
                    for (var mode = 0; mode < 3; mode++) {
                        try {
                            if (mode === 0) eval(source);
                            else if (mode === 1) (0, eval)(source);
                            else Function('return ' + source);
                            print('missing error');
                        } catch (error) {
                            print(error instanceof intrinsicRangeError);
                            print(error.message);
                        }
                        print(eval('1 + 2'));
                    }
                    "#,
                    &["true", crate::stack::STACK_EXHAUSTED_MESSAGE, "3",
                      "true", crate::stack::STACK_EXHAUSTED_MESSAGE, "3",
                      "true", crate::stack::STACK_EXHAUSTED_MESSAGE, "3"],
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn regression_regexp_parser_exhaustion_preserves_syntax_error_and_recovers() {
        std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                assert_output_in_execution_modes(
                    r#"
                    var intrinsicSyntaxError = SyntaxError;
                    var intrinsicRangeError = RangeError;
                    SyntaxError = function() { throw 'replaced'; };
                    RangeError = function() { throw 'replaced'; };
                    var regexpStressDepth = 20000;
                    var source = '['.repeat(regexpStressDepth) + 'a' + ']'.repeat(regexpStressDepth);
                    var receiver = /a/;
                    receiver.lastIndex = 7;
                    for (var mode = 0; mode < 4; mode++) {
                        try {
                            if (mode === 0) new RegExp(source, 'v');
                            else if (mode === 1) eval('/' + source + '/v');
                            else if (mode === 2) Function('return /' + source + '/v');
                            else receiver.compile(source, 'v');
                            print('missing error');
                        } catch (error) {
                            print(error instanceof (mode === 1 || mode === 2 ? intrinsicRangeError : intrinsicSyntaxError));
                            print(error.message.endsWith('Maximum call stack size exceeded'));
                        }
                        print(receiver.source);
                        print(receiver.lastIndex);
                        print(/a/.test('a'));
                    }
                    "#,
                    &[
                        "true", "true", "a", "7", "true",
                        "true", "true", "a", "7", "true",
                        "true", "true", "a", "7", "true",
                        "true", "true", "a", "7", "true",
                    ],
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn regression_dynamic_compiler_exhaustion_is_catchable_and_recovers() {
        std::thread::Builder::new()
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                assert_output_in_execution_modes(
                    r#"
                    var intrinsicRangeError = RangeError;
                    RangeError = function() { throw 'replaced'; };
                    var source = Array(20000).fill('1').join('+');
                    for (var mode = 0; mode < 3; mode++) {
                        try {
                            if (mode === 0) eval(source);
                            else if (mode === 1) (0, eval)(source);
                            else Function('return ' + source);
                            print('missing error');
                        } catch (error) {
                            print(error instanceof intrinsicRangeError);
                            print(error.message);
                        }
                        print(eval('1 + 2'));
                    }
                    "#,
                    &[
                        "true",
                        crate::stack::STACK_EXHAUSTED_MESSAGE,
                        "3",
                        "true",
                        crate::stack::STACK_EXHAUSTED_MESSAGE,
                        "3",
                        "true",
                        crate::stack::STACK_EXHAUSTED_MESSAGE,
                        "3",
                    ],
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn regression_error_bootstrap_preserves_global_binding_order() {
        assert_output_in_execution_modes(
            "print(Object.getOwnPropertyNames(globalThis).filter(name => ['Object', 'Date', 'RangeError', 'RegExp'].includes(name)).join(','));",
            &["Object,Date,RangeError,RegExp"],
        );
    }

    #[test]
    fn regression_runtime_owners_share_recursion_budget() {
        let program = Engine::specialize("print(42);", "reentry.js").unwrap();
        let mut runtimes = [
            Runtime::new(Capture::default()),
            Runtime::new(Capture::default()),
        ];
        let mut guards = Vec::new();
        while let Ok(guard) = quench_stack::StackGuard::enter() {
            guards.push(guard);
        }
        let errors = runtimes
            .iter_mut()
            .map(|runtime| runtime.execute(&program).unwrap_err())
            .collect::<Vec<_>>();
        drop(guards);
        for (runtime, error) in runtimes.iter_mut().zip(errors) {
            assert!(error.thrown_value().is_some());
            assert_eq!(runtime.format_error(&program, &error),
                format!("RangeError: {}", quench_stack::STACK_EXHAUSTED_MESSAGE));
            runtime.execute(&program).unwrap();
        }
    }

    #[test]
    fn regression_recursive_guest_transitions_throw_and_release_stack_budget() {
        std::thread::Builder::new()
            .name("recursion-regression".into())
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                assert_output_in_execution_modes(
                    r#"
                    function check(operation) {
                      try { operation(); print('returned'); }
                      catch (error) {
                        print(error instanceof RangeError);
                        print(error.message);
                      }
                      print((function () { return 42; })());
                    }
                    check(function () { function f() { f(); } f(); });
                    check(function () {
                      var object = { get value() { return this.value; } };
                      object.value;
                    });
                    check(function () {
                      var object = { set value(value) { this.value = value; } };
                      object.value = 1;
                    });
                    check(function () {
                      function C() { new C(); } new C();
                    });
                    check(function () {
                      var proxy = new Proxy({}, {
                        get: function (target, key, receiver) { return receiver[key]; }
                      });
                      proxy.value;
                    });
                    check(function () {
                      var object = { toString: function () { return String(this); } };
                      String(object);
                    });
                    check(function () {
                      var nesting = 10000;
                      JSON.parse('['.repeat(nesting) + '0' + ']'.repeat(nesting));
                    });
                    "#,
                    &[
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                        "true", "Maximum call stack size exceeded", "42",
                    ],
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn regression_stack_errors_preserve_realm_intrinsics_through_collection() {
        std::thread::Builder::new()
            .name("recursion-realm-regression".into())
            .stack_size(crate::WORKER_STACK_SIZE)
            .spawn(|| {
                assert_output_in_execution_modes(
                    r#"
                    print($262.gc.name);
                    print($262.gc.length);
                    var retained = [{ answer: 42 }, /x/];
                    $262.gc();
                    print(retained[0].answer);
                    print(retained[1].test('x'));
                    var intrinsic = RangeError;
                    Object.defineProperty(globalThis, 'RangeError', {
                      get: function () { throw 'guest getter ran'; }, configurable: true
                    });
                    function f() { f(); }
                    try { f(); } catch (error) {
                      print(Object.getPrototypeOf(error) === intrinsic.prototype);
                      var descriptor = Object.getOwnPropertyDescriptor(error, 'message');
                      print(descriptor.value);
                      print(descriptor.writable && !descriptor.enumerable && descriptor.configurable);
                    }
                    var realm = $262.createRealm();
                    var foreignRangeError = realm.global.RangeError;
                    var foreign = realm.evalScript('(function f() { f(); })');
                    realm.evalScript('RangeError = undefined');
                    realm.gc();
                    try { foreign(); } catch (error) {
                      print(Object.getPrototypeOf(error) === foreignRangeError.prototype);
                      print(error.message);
                    }
                    "#,
                    &[
                        "gc", "0", "42", "true", "true",
                        "Maximum call stack size exceeded", "true", "true",
                        "Maximum call stack size exceeded",
                    ],
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn regression_array_buffer_slice_rechecks_source_after_guest_effects() {
        assert_output_in_execution_modes(
            r#"
            for (var position of [0, 1]) {
              var buffer = new ArrayBuffer(8);
              var order = [];
              var start = { valueOf: function () {
                order.push('start');
                if (position === 0) { $262.detachArrayBuffer(buffer); $262.gc(); }
                return 1;
              } };
              var end = { valueOf: function () {
                order.push('end');
                if (position === 1) { $262.detachArrayBuffer(buffer); $262.gc(); }
                return 4;
              } };
              buffer.constructor = { [Symbol.species]: function (length) {
                order.push('species'); return new ArrayBuffer(length);
              } };
              try { buffer.slice(start, end); print('returned'); }
              catch (error) { print(error instanceof TypeError); }
              print(order.join(','));
              print(buffer.byteLength);
            }
            var buffer = new ArrayBuffer(3);
            var view = new Uint8Array(buffer);
            view.set([1, 2, 3]);
            buffer.constructor = { [Symbol.species]: function (length) {
              view[0] = 9; $262.gc(); return new ArrayBuffer(length);
            } };
            var start = { valueOf: function () { view[1] = 8; return 0; } };
            print(new Uint8Array(buffer.slice(start)).join(','));
            var buffer = new ArrayBuffer(4, { maxByteLength: 8 });
            new Uint8Array(buffer).set([1, 2, 3, 4]);
            var start = { valueOf: function () { buffer.resize(2); return 0; } };
            print(new Uint8Array(buffer.slice(start)).join(','));
            "#,
            &["true", "start,end,species", "0", "true", "start,end,species", "0", "9,8,3", "1,2,0,0"],
        );
    }

    #[test]
    fn script_requests_use_the_validated_runtime_boundary() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script("print(40 + 2);", "boundary.js"))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["42"]);
    }

    #[test]
    fn module_requests_use_module_compilation() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest {
                source: "print('module'); export default 1;",
                name: "module.mjs",
                kind: SourceKind::Module,
            })
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["module"]);
    }

    #[test]
    fn arrow_functions_compile_as_residual_closures() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var add = (x) => x + 1; print(add(41));",
                "arrow.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["42"]);
    }

    #[test]
    fn generic_reference_matches_specialized_output() {
        let source = "var proto = { answer: 41 }; var other = Object.create({ answer: 1 }); var object = Object.create(proto); object.add = function(value) { return value + 1; }; function read(value) { return value.answer; } print(read(object) + read(other) + object.add(1));";
        let optimized_host = Capture::default();
        let optimized_view = optimized_host.clone();
        let mut optimized = Runtime::new(optimized_host);
        optimized
            .execute(&Engine::specialize(source, "optimized.js").unwrap())
            .unwrap();

        let generic_host = Capture::default();
        let generic_view = generic_host.clone();
        let mut generic = Runtime::new(generic_host);
        generic
            .execute(&Engine::specialize_unspecialized(source, "generic.js").unwrap())
            .unwrap();

        assert_eq!(
            optimized_view.0.borrow().as_slice(),
            generic_view.0.borrow().as_slice()
        );
    }

    #[test]
    fn wide_instruction_side_table_executes_through_the_general_core() {
        let source = format!("print([{}].length);", "0,".repeat(4097));
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        let program = Engine::specialize(&source, "wide.js").unwrap();
        assert!(
            program
                .functions
                .iter()
                .any(|function| !function.wide.is_empty())
        );
        let path = std::env::temp_dir().join(format!("rqj-wide-{}", std::process::id()));
        program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(
            decoded
                .functions
                .iter()
                .any(|function| !function.wide.is_empty())
        );
        runtime.execute(&decoded).unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["4097"]);
    }

    #[test]
    fn oxc_lone_surrogate_escapes_retain_utf16_units() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        let program = Engine::compile(ExecutionRequest::script(
            r#"print("\uD800".length); print("\uD800".charCodeAt(0));"#,
            "surrogate.js",
        ))
        .unwrap();
        let path = std::env::temp_dir().join(format!("rqj-surrogate-{}", std::process::id()));
        program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        runtime.execute(&decoded).unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["1", "55296"]);
    }

    #[test]
    fn oxc_surrogate_property_keys_use_exact_index_semantics() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                r#"var object = {"\uD800": 1}; print(Object.keys(object)[0].charCodeAt(0)); print(object["\uD800"]);"#,
                "surrogate-key.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["55296", "1"]);
    }

    #[test]
    fn oxc_surrogate_keys_cover_class_fields_and_destructuring() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                r#"class Box { "\uD801" = 7; } print(new Box()["\uD801"]); var {"\uD800": value} = {"\uD800": 3}; print(value);"#,
                "surrogate-class-destructure.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["7", "3"]);
    }

    #[test]
    fn finalization_registry_registers_and_unregisters_generation_checked_tokens() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var registry = new FinalizationRegistry(function() {}); var target = {}; var token = {}; registry.register(target, 1, token); print(registry.unregister(token)); print(registry.unregister(token));",
                "finalization.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["true", "false"]);
    }

    #[test]
    fn finalization_registry_uses_generic_property_traversal() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        let program = Engine::specialize_unspecialized(
            "var registry = new FinalizationRegistry(function() {}); var token = {}; registry.register({}, 1, token); print(typeof registry.unregister); print(registry.unregister(token));",
            "finalization-generic.js",
        )
        .unwrap();
        runtime.execute(&program).unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["function", "true"]);
    }

    #[test]
    fn finalization_registry_drains_collected_targets_after_their_root_is_cleared() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var registry = new FinalizationRegistry(function(held) { print(held); }); var target = {}; registry.register(target, 7); target = undefined; for (var i = 0; i < 10000; i = i + 1) { var temporary = {}; } print(\"done\");",
                "finalization-drain.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["done", "7"]);
    }

    #[test]
    fn prototype_mutation_rejects_cycles_and_invalidates_property_caches() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var first = { value: 1 }; var second = Object.create(first); print(second.value); var third = { value: 3 }; Object.setPrototypeOf(second, third); print(second.value); try { Object.setPrototypeOf(third, second); print(\"not-rejected\"); } catch (error) { print(\"cycle\"); }",
                "prototype-cycle.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["1", "3", "cycle"]);
    }

    #[test]
    fn stale_root_cannot_enqueue_a_job() {
        let mut runtime = Runtime::new(Capture::default());
        let root = runtime.root(Value::UNDEFINED);
        assert!(runtime.release_root(root));
        assert!(!runtime.enqueue_rooted_job(root, &[]));
    }

    #[test]
    fn root_handles_cannot_cross_runtime_boundaries() {
        let mut owner = Runtime::new(Capture::default());
        let mut other = Runtime::new(Capture::default());
        let owned_root = owner.root(Value::number(1.0));
        let other_root = other.root(Value::number(2.0));

        assert!(!other.root_is_live(owned_root));
        assert!(!other.update_root(owned_root, Value::number(3.0)));
        assert!(!other.release_root(owned_root));
        assert!(!other.enqueue_rooted_job(owned_root, &[]));
        assert!(other.root_is_live(other_root));
    }

    #[test]
    fn descriptor_transitions_reject_mixed_fields_and_allow_configurable_kind_changes() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var object = {}; try { Object.defineProperty(object, 'mixed', { value: 1, get: function() { return 2; } }); print('bad'); } catch (error) { print('mixed'); } Object.defineProperty(object, 'value', { get: function() { return 3; }, configurable: true }); Object.defineProperty(object, 'value', { value: 4 }); print(object.value); var array = []; Object.defineProperty(array, '0', { get: function() { return 5; }, configurable: true }); Object.defineProperty(array, '0', { value: 6 }); print(array[0]);",
                "descriptor-transition.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["mixed", "4", "6"]);
    }

    #[test]
    fn define_properties_uses_one_snapshot_of_descriptor_keys() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var target = {}; var descriptors = { a: { value: 1 }, b: { value: 2, enumerable: true } }; Object.defineProperties(target, descriptors); print(target.a); print(target.b); print(Object.keys(target).join(',')); var created = Object.create(null, { x: { value: 9, enumerable: true } }); print(created.x); print(Object.keys(created).join(','));",
                "define-properties.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["1", "2", "b", "9", "x"]);
    }

    #[test]
    fn explicit_collection_preserves_runtime_roots() {
        let mut runtime = Runtime::new(Capture::default());
        let program = Engine::specialize("print(0);", "collect.js").unwrap();
        runtime.execute(&program).unwrap();
        let root = runtime.root(Value::number(42.0));
        runtime.collect(&program).unwrap();
        assert!(runtime.root_is_live(root));
        assert!(runtime.release_root(root));
    }

    #[test]
    fn array_length_descriptor_is_an_own_non_enumerable_property() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var descriptor = Object.getOwnPropertyDescriptor([1, 2], 'length'); print(descriptor.value); print(descriptor.enumerable); print(descriptor.configurable);",
                "array-length-descriptor.js",
            ))
            .unwrap();
        assert_eq!(view.0.borrow().as_slice(), ["2", "false", "false"]);
    }

    #[test]
    fn array_length_is_visible_only_to_non_enumerable_own_key_views() {
        let host = Capture::default();
        let view = host.clone();
        let mut runtime = Runtime::new(host);
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "var array = [1, 2]; print(Object.keys(array).join(',')); print(Object.getOwnPropertyNames(array).join(',')); print(Reflect.ownKeys(array).join(','));",
                "array-own-keys.js",
            ))
            .unwrap();
        assert_eq!(
            view.0.borrow().as_slice(),
            ["0,1", "0,1,length", "0,1,length"]
        );
    }

    #[test]
    fn rooted_execution_completion_survives_collection() {
        let mut runtime = Runtime::new(Capture::default());
        let program = Engine::specialize("print('ran');", "rooted-result.js").unwrap();
        let completion = runtime.execute_rooted(&program).unwrap();

        assert!(runtime.root_is_live(completion));
        runtime.collect(&program).unwrap();
        assert!(runtime.root_is_live(completion));
        assert!(runtime.release_root(completion));
        assert!(!runtime.root_is_live(completion));
    }

    #[test]
    fn regression_eval_defaults_capture_caller_and_infer_function_names() {
        assert_output_in_execution_modes(
            r#"
            function caller() {
                var offset = 39;
                function value() { return offset + 3; }
                eval('var f = function named([a = value(), ...rest]) { return a; };');
                print(f([]));
                print(f.name);
                eval('var inferred = function() {};');
                print(inferred.name);
            }
            caller();
            "#,
            &["42", "named", "inferred"],
        );
    }

    #[test]
    fn regression_eval_global_bindings_follow_property_descriptors() {
        assert_output_in_execution_modes(
            r#"
            var initial;
            var x = 23;
            eval('initial = x; var x = 45;');
            print(initial);
            print(x);
            var descriptor = Object.getOwnPropertyDescriptor(this, 'x');
            print(descriptor.value);
            print(descriptor.writable);
            print(descriptor.enumerable);
            print(descriptor.configurable);
            var reads = 0;
            var writes = 0;
            eval('var visible = 1; Object.defineProperty(this, "visible", {get: function() { reads++; return 71; }, set: function(v) { writes = v; }}); print(visible); visible = 12; print(writes); print(visible);');
            print(reads);
            var frozen = 2;
            Object.defineProperty(this, 'frozen', {writable: false});
            frozen = 5;
            print(frozen);
            print(eval('frozen'));
            try { eval('"use strict"; frozen = 7;'); }
            catch (error) { print(error.name); }
            "#,
            &[
                "23",
                "45",
                "45",
                "true",
                "true",
                "false",
                "71",
                "12",
                "71",
                "2",
                "2",
                "2",
                "TypeError",
            ],
        );
    }

    #[test]
    fn regression_eval_property_sites_use_the_executing_program() {
        assert_output_in_execution_modes(
            r#"
            eval('var object = {a: 2, b: 3, c: 4}; object.a = object.b + object.c; print(object.a); print(object.b); print(object.c);');
            "#,
            &["7", "3", "4"],
        );
    }

    #[test]
    fn regression_regexp_literals_and_species_use_realm_intrinsics() {
        assert_output_in_execution_modes(
            r#"
            var original = RegExp;
            RegExp = null;
            var expression = /a/g;
            print(Object.getPrototypeOf(expression) === original.prototype);
            print(original(expression) === expression);
            expression.constructor = undefined;
            print(expression[Symbol.matchAll]('a').next().value[0]);
            print(expression[Symbol.split]('ba')[0]);
            function shadowed(RegExp) { return /b/.test('b'); }
            print(shadowed(null));
            "#,
            &["true", "true", "a", "b", "true"],
        );
    }

    #[test]
    fn regression_foreign_regexp_realm_has_intrinsic_methods_and_descriptors() {
        assert_output_in_execution_modes(
            r#"
            var other = $262.createRealm().global;
            other.eval('var intrinsic = RegExp; RegExp = null; var expression = /a/g;');
            print(Object.getPrototypeOf(other.expression) === other.intrinsic.prototype);
            print(other.expression.toString());
            print(other.expression.test('a'));
            var descriptor = Object.getOwnPropertyDescriptor(other.intrinsic, 'prototype');
            print(descriptor.writable);
            print(descriptor.enumerable);
            print(descriptor.configurable);
            "#,
            &["true", "/a/g", "true", "false", "false", "false"],
        );
    }

    #[test]
    fn regression_regexp_constructor_uses_internal_source_and_flags() {
        assert_output_in_execution_modes(
            r#"
            var expression = /a/g;
            var gets = 0;
            Object.defineProperty(expression, 'source', {get: function() { gets++; return 'wrong'; }});
            Object.defineProperty(expression, 'flags', {get: function() { gets++; return 'i'; }});
            var cloned = new RegExp(expression);
            print(cloned.source);
            print(cloned.flags);
            print(gets);
            "#,
            &["a", "g", "0"],
        );
    }

    #[test]
    fn regression_regexp_constructor_orders_getters_prototype_and_coercions() {
        assert_output_in_execution_modes(
            r#"
            var events = [];
            var like = {};
            Object.defineProperty(like, Symbol.match, {get: function() { events.push('match'); return true; }});
            Object.defineProperty(like, 'source', {get: function() { events.push('source'); return {toString: function() { events.push('source coercion'); return 'a'; }}; }});
            Object.defineProperty(like, 'flags', {get: function() { events.push('flags'); return {toString: function() { events.push('flags coercion'); return 'g'; }}; }});
            var target = new Proxy(function() {}, {get: function(t, key) { if (key === 'prototype') events.push('prototype'); return Reflect.get(t, key); }});
            var result = Reflect.construct(RegExp, [like], target);
            print(events.join(','));
            print(result.source);
            print(result.flags);
            "#,
            &[
                "match,source,flags,prototype,source coercion,flags coercion",
                "undefined",
                "undefined",
            ],
        );
    }

    #[test]
    fn regression_legacy_braced_unicode_uses_identity_escape_and_quantifier() {
        assert_output_in_execution_modes(
            r#"
            print(/\u{3}/.exec('Auuu')[0]);
            print(/\u{41}/u.exec('Auuu')[0]);
            print(/\u{3}/.test('uuu'));
            print(/\u{3}/.test('A'));
            print(/\u{4A}/.exec('u{4A}')[0]);
            "#,
            &["uuu", "A", "true", "false", "u{4A}"],
        );
    }

    #[test]
    fn regression_regexp_source_escapes_line_terminators_and_preserves_classes() {
        assert_output_in_execution_modes(
            r#"
            var rows = [
                ['\n', '\\n'], ['\\\n', '\\n'], ['\\\\\n', '\\\\\\n'],
                ['\r', '\\r'], ['\\\r', '\\r'],
                ['\u2028', '\\u2028'], ['\\\u2028', '\\u2028'],
                ['\u2029', '\\u2029'], ['\\\u2029', '\\u2029'],
                ['/', '\\/'], ['[/]', '[/]'], ['[\\/]', '[\\/]'],
                ['\\[/\\]', '\\[\\/\\]']
            ];
            var passed = 0;
            for (var row of rows) {
                var expression = new RegExp(row[0]);
                if (expression.source === row[1]) passed++;
                if (eval('/' + expression.source + '/').source === row[1]) passed++;
                if (expression.toString() === '/' + row[1] + '/') passed++;
            }
            print(passed);
            "#,
            &["39"],
        );
    }

    #[test]
    fn regression_regexp_preserves_raw_utf16_patterns_across_entry_points() {
        assert_output_in_execution_modes(
            r#"
            var lone = '\uD83D';
            var pair = '\uD83D\uDC38';
            print(new RegExp(lone, 'u').exec(lone)[0].charCodeAt(0));
            print(new RegExp(lone, 'u').source.charCodeAt(0));
            print(eval('/' + lone + '/u').exec(lone)[0].charCodeAt(0));
            print(eval('/[' + pair + ']/').exec(pair)[0].charCodeAt(0));
            print(eval('/[' + pair + ']/u').exec(pair)[0].length);
            print(eval('/' + pair + '?/').exec('') === null);
            print(new RegExp(pair + '?').exec('') === null);
            print(new RegExp('\\uD83D\uDC38', 'u').test(pair));
            print(new RegExp('\uD83D\\uDC38', 'u').test(pair));
            print(new RegExp('\\uD83D\uDC38').test(pair));
            print(new RegExp('\uD83D\\uDC38').test(pair));
            print(new RegExp('\\' + lone).test(lone));
            try { new RegExp('\\' + lone, 'u'); }
            catch (error) { print(error.name); }
            var expression = /a/;
            expression.compile('[' + lone + ']', 'u');
            print(expression.exec(lone)[0].charCodeAt(0));
            print(new RegExp(expression).exec(lone)[0].charCodeAt(0));
            "#,
            &[
                "55357",
                "55357",
                "55357",
                "55357",
                "2",
                "true",
                "true",
                "false",
                "false",
                "true",
                "true",
                "true",
                "SyntaxError",
                "55357",
                "55357",
            ],
        );
    }

    #[test]
    fn regression_regexp_control_letters_work_in_classes_and_ranges() {
        assert_output_in_execution_modes(
            r#"
            print(/[\cA]/u.test('\u0001'));
            print(/[\cz]/u.exec('\u001A')[0].charCodeAt(0));
            print(/[\cA-\cZ]/u.test('\u0007'));
            print(/[\cA-\cZ]/u.test('A'));
            print(/[\cA]/v.test('\u0001'));
            print(/[\cA]/.test('\u0001'));
            print(/[\\cA]/u.test('c'));
            "#,
            &["true", "26", "true", "false", "true", "true", "true"],
        );
    }

    #[test]
    fn regression_eval_regexp_class_delimiters_share_literal_scanning() {
        assert_output_in_execution_modes(
            r#"
            print(eval(' /[/]/ ').source);
            print(eval(' /[/]/ ').test('/'));
            print(eval('(/[/]/)').source);
            "#,
            &["[/]", "true", "[/]"],
        );
    }

    #[test]
    fn regression_eval_name_reads_do_not_borrow_bytecode_cache_sites() {
        assert_output_in_execution_modes(
            r#"
            print(eval('(42)'));
            print(eval('({answer: 42})').answer);
            var expression = eval('(/a/)');
            print(expression.source);
            print(expression.test('a'));
            var visible = 7;
            print(eval('visible'));
            "#,
            &["42", "42", "a", "true", "7"],
        );
    }

    #[test]
    fn regression_intl_german_date_format_ignores_string_split_override() {
        assert_output_in_execution_modes(
            r#"
            var possibleAnswers = ["1.1.1970", "2.1.1970", "3.1.1970"];
            var replacements = ["", "x-foo", "de-u-co", "en-US"];
            for (var index = 0; index < replacements.length; index++) {
                String.prototype[Symbol.split] = function() { return [replacements[index]]; };
                var formatted = Intl.DateTimeFormat("de", {}).format(86400000);
                print(formatted);
                print(possibleAnswers.includes(formatted));
            }
            "#,
            &[
                "1.1.1970",
                "true",
                "1.1.1970",
                "true",
                "1.1.1970",
                "true",
                "1.1.1970",
                "true",
            ],
        );
    }

    #[test]
    fn regression_arguments_binding_survives_closure_and_direct_eval() {
        assert_output_in_execution_modes(
            r#"
            function countArguments(a, b, c, d) {
                return (() => arguments.length)();
            }
            function evalArgumentsLength() {
                eval("arguments.length = 42");
                return arguments.length;
            }
            print(countArguments());
            print(countArguments(1, 2, 3));
            print(countArguments(1, 2, 3, 4));
            print(evalArgumentsLength());
            "#,
            &["0", "3", "4", "42"],
        );
    }

}

#[cfg(test)]
#[path = "api_promise_tests.rs"]
mod promise_tests;

#[cfg(test)]
#[path = "api_iterator_tests.rs"]
mod iterator_tests;
