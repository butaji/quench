//! Test262 capability adapter for the canonical shared runtime.

use std::path::Path;

use quench_runtime::{
    CapabilityId, Engine, ExecutionMode, ExecutionRequest, Host, HostFunction, HostFunctionId,
    HostGlobal, ModuleSource, NativeContext, RootId, RootedError, Runtime, SourceKind, SystemHost,
};
use std::time::{Duration, Instant};

use crate::Test262Host;

fn compile_failure(errors: &[quench_runtime::Diagnostic]) -> String {
    if !errors.is_empty() && errors.iter().all(quench_runtime::Diagnostic::is_syntax_error) {
        format!("quench runtime SyntaxError: {errors:?}")
    } else {
        format!("quench compilation failed: {errors:?}")
    }
}

const TEST262_MODULE_SOURCE_SPECIFIER: &str = "<module source>";
const HARNESS_SCRIPT_NAME: &str = "<test262-harness>";
const TIMER_MIN_DELAY_MS: f64 = 1.0;
const TIMER_MAX_DELAY_MS: f64 = 2_147_483_647.0;

#[cfg(feature = "aot")]
fn native_output_directory(name: &str, variant: &str) -> Result<Option<std::path::PathBuf>, String> {
    let Some(root) = std::env::var_os("TEST262_AOT_OUTPUT_DIR") else { return Ok(None); };
    let suite = std::env::var_os("TEST262_DIR").map(std::path::PathBuf::from)
        .unwrap_or_else(|| "tests/test262".into()).join("test");
    let path = Path::new(name);
    let relative = path.strip_prefix(&suite).unwrap_or_else(|_| Path::new(path.file_name().unwrap_or_default()));
    let directory = std::path::PathBuf::from(root).join(relative).join(variant);
    std::fs::create_dir_all(&directory).map_err(|error| format!("quench AOT output: {error}"))?;
    Ok(Some(directory))
}

#[cfg(feature = "aot")]
fn compile_native_unit(
    program: quench_runtime::ResidualProgram,
    compilation: &mut crate::NativeCompilation,
    output: Option<&Path>,
) -> Result<quench_runtime::NativeJsProgram<Test262RuntimeHost>, String> {
    let name = format!("test262_unit_{}", compilation.units.len());
    let module = quench_runtime::lower_native_js_module(&program, &name).map_err(|error| format!("quench AOT lowering: {error}"))?;
    let object = quench_runtime::emit_native_module(&name, module.functions, module.data)
        .map_err(|error| format!("quench AOT object: {error}"))?;
    let object_bytes = object.len();
    let path = output.map(|directory| directory.join(format!("{name}.o")));
    if let Some(path) = &path {
        std::fs::write(path, object).map_err(|error| format!("quench AOT object write: {error}"))?;
    }
    let functions = program.function_count();
    let native = quench_runtime::NativeJsProgram::compile_eager(program).map_err(|error| format!("quench AOT publication: {error}"))?;
    compilation.units.push(crate::NativeCompiledUnit { functions, object_bytes, object: path });
    Ok(native)
}

static HOST_GLOBALS: [HostGlobal; 2] = [
    HostGlobal {
        name: "$262",
        capability: CapabilityId::CreateRealm,
    },
    HostGlobal {
        name: "$262",
        capability: CapabilityId::IsHTMLDDA,
    },
];
static ASYNC_GLOBALS: [HostGlobal; 3] = [
    HOST_GLOBALS[0],
    HOST_GLOBALS[1],
    HostGlobal {
        name: "$DONE",
        capability: CapabilityId::Done,
    },
];

#[derive(Debug, Default)]
pub struct Test262RuntimeHost {
    async_test: bool,
    can_block: bool,
    done: Option<String>,
    /// The execution every case's runtime uses: the interpreter, or compiled (JIT) execution.
    execution_mode: ExecutionMode,
    native_compilation: crate::NativeCompilation,
    native_variant: &'static str,
    timers: Vec<TestTimer>,
    next_timer_id: u64,
}

#[derive(Debug)]
struct TestTimer {
    id: u64,
    callback: RootId,
    args: Vec<RootId>,
    deadline: Instant,
}

impl Host for Test262RuntimeHost {
    fn write_line(&mut self, text: &str) {
        if let Some(error) = text.strip_prefix("Test262:AsyncTestFailure:") {
            self.done = Some(error.to_string());
            return;
        }
        if text == "Test262:AsyncTestComplete" {
            self.done = Some(String::new());
            return;
        }
        println!("{text}");
    }

    fn clock_millis(&mut self) -> f64 {
        SystemHost.clock_millis()
    }

    fn globals(&self) -> &'static [HostGlobal] {
        if self.async_test {
            &ASYNC_GLOBALS
        } else {
            &HOST_GLOBALS
        }
    }

    fn done(&mut self, text: Option<&str>) {
        self.done = Some(text.unwrap_or_default().to_string());
    }

    fn has_module_source(&self, module: &ModuleSource) -> bool {
        module.name == TEST262_MODULE_SOURCE_SPECIFIER
    }

    fn can_block(&self) -> bool {
        self.can_block
    }

    fn functions(&self) -> &[HostFunction<Self>] {
        quench_runtime::host_functions![
            method "test262SetTimeout" (2) => set_timeout,
            method "test262ClearTimeout" (1) => clear_timeout,
        ]
    }

    fn initialize(context: &mut NativeContext<'_, Self>) -> Result<(), RootedError> {
        if !context.host_mut().async_test {
            return Ok(());
        }
        let global = context.global_root()?;
        for (name, id) in [
            ("setTimeout", HostFunctionId(0)),
            ("clearTimeout", HostFunctionId(1)),
        ] {
            let key = context.string_rooted(name);
            let function = context.host_function(id)?;
            if !context.define_data_property_rooted(global, key, function, true, false, true)? {
                return Err(RootedError::host(format!("cannot install Test262 {name}")));
            }
        }
        Ok(())
    }

    fn resolve_dynamic_import(
        &mut self,
        referrer: &str,
        specifier: &str,
    ) -> Result<Option<ModuleSource>, quench_runtime::ModuleLoadError> {
        if specifier == TEST262_MODULE_SOURCE_SPECIFIER {
            return Ok(Some(ModuleSource::text(specifier, "")));
        }
        if !specifier.starts_with('.') {
            return Ok(None);
        }
        let referrer = Path::new(referrer);
        let path = referrer
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .join(specifier);
        let bytes =
            std::fs::read(&path).map_err(|error| format!("module {}: {error}", path.display()))?;
        Ok(Some(ModuleSource::from_bytes(
            path.display().to_string(),
            bytes,
        )))
    }
}

impl Test262RuntimeHost {
    pub fn new(execution_mode: ExecutionMode) -> Self {
        Self {
            execution_mode,
            ..Self::default()
        }
    }

    fn execute(&mut self, source: &str, name: &str) -> Result<(), String> {
        self.execute_kind(source, name, SourceKind::Script)
    }

    fn execute_kind(&mut self, source: &str, name: &str, kind: SourceKind) -> Result<(), String> {
        self.execute_after_harness(&[], source, name, kind)
    }

    /// Harness files are global Scripts evaluated in the test's realm before
    /// its entry, so modules the entry loads observe the same harness bindings.
    fn execute_after_harness(
        &mut self,
        harness: &[&str],
        source: &str,
        name: &str,
        kind: SourceKind,
    ) -> Result<(), String> {
        self.done = None;
        let async_test = self.async_test;
        let execution_mode = self.execution_mode;
        let mut runtime = Runtime::new(std::mem::take(self));
        runtime.set_execution_mode(execution_mode);
        let result = (|| {
            #[cfg(feature = "aot")]
            let mut compiler = quench_runtime::NativeJsCompiler::new(false);
            #[cfg(feature = "aot")]
            let mut native_harness = Vec::new();
            #[cfg(feature = "aot")]
            if execution_mode == ExecutionMode::NativeOnly {
                for script in harness {
                    native_harness.push(compiler.compile_script(script, HARNESS_SCRIPT_NAME, Default::default())
                        .map_err(|errors| format!("quench AOT harness SyntaxError: {errors:?}"))?);
                }
            }
            let program = if execution_mode == ExecutionMode::NativeOnly {
                #[cfg(feature = "aot")]
                {
                    let program = match kind {
                        SourceKind::Module => compiler.compile_module(source, name),
                        SourceKind::Script => compiler.compile_script(source, name, Default::default()),
                        SourceKind::Eval => return Err("quench AOT: eval source goal is not a static entry".into()),
                    }.map_err(|errors| compile_failure(&errors))?;
                    compiler.finish(program).map_err(|error| format!("quench AOT namespace: {error}"))?
                }
                #[cfg(not(feature = "aot"))]
                { return Err("quench AOT code generation requires the aot feature".into()); }
            } else { match kind {
                SourceKind::Script => Engine::specialize_unspecialized(source, name),
                SourceKind::Module => Engine::specialize_module_unspecialized(source, name),
                SourceKind::Eval => Engine::compile(ExecutionRequest { source, name, kind }),
            }
            .map_err(|errors| compile_failure(&errors))? };
            let harness = harness
                .iter()
                .map(|script| (*script, HARNESS_SCRIPT_NAME))
                .collect::<Vec<_>>();
            let execution = if execution_mode == ExecutionMode::NativeOnly {
                #[cfg(feature = "aot")]
                {
                    let host = runtime.host_mut();
                    let output = native_output_directory(name, host.native_variant)?;
                    let mut scripts = Vec::new();
                    for script in native_harness {
                        scripts.push(compile_native_unit(script, &mut host.native_compilation, output.as_deref())?);
                    }
                    let native = compile_native_unit(program.clone(), &mut host.native_compilation, output.as_deref())?;
                    runtime.execute_native_after_scripts(&scripts, &native)
                }
                #[cfg(not(feature = "aot"))]
                { unreachable!("AOT feature checked before compilation") }
            } else { runtime.execute_after_scripts(&harness, &program) };
            execution
                .map_err(|error| {
                    format!("quench runtime: {}", runtime.format_error(&program, &error))
                })?;
            if async_test {
                loop {
                    runtime.run_jobs(&program).map_err(|error| {
                        format!(
                            "quench runtime jobs: {}",
                            runtime.format_error(&program, &error)
                        )
                    })?;
                    if let Some(error) = runtime.host_mut().done.clone() {
                        if !error.is_empty() {
                            return Err(format!("quench runtime async: {error}"));
                        }
                        break;
                    }
                    let Some(timer) = runtime.host_mut().take_next_timer() else {
                        return Err("quench runtime async: $DONE was not called".into());
                    };
                    let args = timer.args;
                    let queued = runtime.enqueue_rooted_job(timer.callback, &args);
                    runtime.release_root(timer.callback);
                    for arg in args {
                        runtime.release_root(arg);
                    }
                    if !queued {
                        return Err("quench runtime async: timer callback root expired".into());
                    }
                }
            }
            Ok(())
        })();
        for timer in runtime.host_mut().take_all_timers() {
            runtime.release_root(timer.callback);
            for arg in timer.args {
                runtime.release_root(arg);
            }
        }
        let rejections = runtime.native_execution_rejections();
        runtime.host_mut().native_compilation.rejections = rejections;
        *self = runtime.into_host();
        if rejections != 0 {
            Err(format!("quench AOT capability rejection: {rejections} native code requests unavailable; outcome: {result:?}"))
        } else { result }
    }

    fn execute_harnessed_script(
        &mut self, harness: &[&str], source: &str, strict: bool, name: &str,
    ) -> Result<(), String> {
        let source = if strict {
            let prefix = if source.starts_with("#!") {
                source.char_indices().find(|(_, ch)| oxc_syntax::line_terminator::is_line_terminator(*ch))
                    .map_or(source.len(), |(index, ch)| index + ch.len_utf8())
            } else { 0 };
            std::borrow::Cow::Owned(format!("{}\n\"use strict\";\n{}", &source[..prefix], &source[prefix..]))
        } else { std::borrow::Cow::Borrowed(source) };
        self.execute_after_harness(harness, &source, name, SourceKind::Script)
    }

}

impl Test262Host for Test262RuntimeHost {
    fn configure(&mut self, metadata: &crate::TestMetadata) {
        self.async_test = metadata.is_async;
        self.can_block = metadata.can_block;
        self.native_compilation = crate::NativeCompilation::default();
        self.native_variant = if metadata.is_module { "module" } else if metadata.is_raw { "raw" } else if metadata.only_strict { "strict" } else { "sloppy" };
    }

    fn native_compilation(&self) -> Option<&crate::NativeCompilation> {
        (self.execution_mode == ExecutionMode::NativeOnly).then_some(&self.native_compilation)
    }

    fn run_script(&mut self, source: &str) -> Result<(), String> {
        self.execute(source, "<test262>")
    }

    fn run_module_script(&mut self, source: &str) -> Result<(), String> {
        self.execute_kind(source, "<test262-module>", SourceKind::Module)
    }

    fn run_harnessed_script(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
    ) -> Result<(), String> {
        self.execute_harnessed_script(harness, source, strict, "<test262>")
    }

    fn run_harnessed_script_at(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
        path: &Path,
    ) -> Result<(), String> {
        self.execute_harnessed_script(harness, source, strict, &path.display().to_string())
    }

    fn run_harnessed_module(&mut self, harness: &[&str], source: &str) -> Result<(), String> {
        self.execute_after_harness(harness, source, "<test262-module>", SourceKind::Module)
    }

    fn run_harnessed_module_at(
        &mut self,
        harness: &[&str],
        source: &str,
        path: &Path,
    ) -> Result<(), String> {
        self.execute_after_harness(
            harness,
            source,
            &path.display().to_string(),
            SourceKind::Module,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::Test262RuntimeHost;

    #[test]
    fn strict_call_assignment_is_a_syntax_error_not_a_backend_rejection() {
        let errors = super::Engine::specialize("'use strict'; f() = 1;", "strict-call-assignment.js").unwrap_err();
        assert!(super::compile_failure(&errors).starts_with("quench runtime SyntaxError:"));
    }

    #[test]
    fn strict_test_does_not_make_harness_functions_strict() {
        let modes = [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled].into_iter();
        #[cfg(feature = "aot")]
        let modes = modes.chain([super::ExecutionMode::NativeOnly]);
        for mode in modes {
            let mut host = Test262RuntimeHost::new(mode);
            host.execute_harnessed_script(
                &["const harnessNumber = 7; function harnessThis() { return this; }"],
                "if (harnessNumber !== 7) throw Error('missing lexical'); if (harnessThis() !== globalThis) throw Error('strict harness'); if ((function() { return this; })() !== undefined) throw Error('sloppy test');",
                true, "strict-entry.js",
            ).unwrap();
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_qualification_rejects_caught_missing_source_compilation() {
        let mut host = Test262RuntimeHost::new(super::ExecutionMode::NativeOnly);
        let result = host.execute("try { eval('1'); } catch (error) {} ", "caught-eval.js");
        assert!(result.unwrap_err().starts_with("quench AOT capability rejection:"));
        assert_eq!(host.native_compilation.rejections, 1);
    }

    #[test]
    fn async_timer_is_a_macrotask_and_then_reactions_stay_microtasks() {
        let mut host = Test262RuntimeHost::default();
        host.async_test = true;
        host.execute(
            r#"
              const order = [];
              const canceled = setTimeout(() => { throw new Error('canceled timer ran'); }, 0);
              clearTimeout(canceled);
              setTimeout(() => {
                order.push('timer');
                Promise.resolve().then(() => {
                  order.push('timer-reaction');
                  if (order.join(',') !== 'initial-reaction,timer,timer-reaction') {
                    throw new Error(order.join(','));
                  }
                  $DONE();
                });
              }, 0);
              Promise.resolve().then(() => order.push('initial-reaction'));
            "#,
            "test262-timer-order.js",
        )
        .unwrap();
    }
}

impl Test262RuntimeHost {
    fn take_next_timer(&mut self) -> Option<TestTimer> {
        let (index, deadline) = self
            .timers
            .iter()
            .enumerate()
            .min_by_key(|(_, timer)| (timer.deadline, timer.id))
            .map(|(index, timer)| (index, timer.deadline))?;
        let delay = deadline.saturating_duration_since(Instant::now());
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
        Some(self.timers.remove(index))
    }

    fn cancel_timer(&mut self, id: u64) -> Option<TestTimer> {
        let index = self.timers.iter().position(|timer| timer.id == id)?;
        Some(self.timers.remove(index))
    }

    fn take_all_timers(&mut self) -> Vec<TestTimer> {
        std::mem::take(&mut self.timers)
    }
}

fn set_timeout(
    context: &mut NativeContext<'_, Test262RuntimeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(callback) = args.first().copied() else {
        let error = context.type_error_rooted("setTimeout callback is not callable")?;
        return Err(context.throw(error));
    };
    if !context.is_callable_rooted(callback)? {
        let error = context.type_error_rooted("setTimeout callback is not callable")?;
        return Err(context.throw(error));
    }
    let delay = args
        .get(1)
        .copied()
        .map(|value| context.to_number(value))
        .transpose()?
        .unwrap_or(0.0);
    let delay = if !delay.is_finite() || delay <= 0.0 {
        TIMER_MIN_DELAY_MS
    } else {
        delay.clamp(TIMER_MIN_DELAY_MS, TIMER_MAX_DELAY_MS)
    };
    let callback = context.retain(callback)?;
    let mut retained_args = Vec::with_capacity(args.len().saturating_sub(2));
    for arg in args.iter().skip(2).copied() {
        retained_args.push(context.retain(arg)?);
    }
    let host = context.host_mut();
    let id = host.next_timer_id;
    host.next_timer_id = id
        .checked_add(1)
        .ok_or_else(|| RootedError::host("Test262 timer identifier space exhausted"))?;
    host.timers.push(TestTimer {
        id,
        callback,
        args: retained_args,
        deadline: Instant::now() + Duration::from_secs_f64(delay / 1000.0),
    });
    Ok(context.number(id as f64))
}

fn clear_timeout(
    context: &mut NativeContext<'_, Test262RuntimeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let id = args
        .first()
        .copied()
        .map(|value| context.to_number(value))
        .transpose()?;
    if let Some(id) = id.filter(|id| id.is_finite() && *id >= 0.0) {
        if let Some(timer) = context.host_mut().cancel_timer(id as u64) {
            context.release_root(timer.callback);
            for arg in timer.args {
                context.release_root(arg);
            }
        }
    }
    Ok(context.undefined())
}
