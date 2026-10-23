//! Test262 capability adapter for the canonical shared runtime.

use std::path::Path;

use quench_runtime::{
    CapabilityId, Engine, ExecutionMode, ExecutionRequest, Host, HostFunction, HostFunctionId,
    HostGlobal, ModuleSource, NativeContext, RootId, RootedError, Runtime, SourceKind, SystemHost,
};
use std::time::{Duration, Instant};

use crate::Test262Host;

const TEST262_MODULE_SOURCE_SPECIFIER: &str = "<module source>";
const HARNESS_SCRIPT_NAME: &str = "<test262-harness>";
const TIMER_MIN_DELAY_MS: f64 = 1.0;
const TIMER_MAX_DELAY_MS: f64 = 2_147_483_647.0;

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
            let program = match kind {
                SourceKind::Script => Engine::specialize_unspecialized(source, name),
                SourceKind::Module => Engine::specialize_module_unspecialized(source, name),
                SourceKind::Eval => Engine::compile(ExecutionRequest { source, name, kind }),
            }
            .map_err(|errors| format!("quench runtime SyntaxError: {errors:?}"))?;
            let harness = harness
                .iter()
                .map(|script| (*script, HARNESS_SCRIPT_NAME))
                .collect::<Vec<_>>();
            runtime
                .execute_after_scripts(&harness, &program)
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
        *self = runtime.into_host();
        result
    }

    fn compose(harness: &[&str], source: &str, strict: bool) -> String {
        let mut composed = String::new();
        if strict {
            composed.push_str("\"use strict\";\n");
        }
        if source.starts_with("#!") {
            composed.push_str(source);
            composed.push('\n');
        }
        for script in harness {
            composed.push_str(script);
            composed.push('\n');
        }
        if !source.starts_with("#!") {
            composed.push_str(source);
        }
        composed
    }
}

impl Test262Host for Test262RuntimeHost {
    fn configure(&mut self, metadata: &crate::TestMetadata) {
        self.async_test = metadata.is_async;
        self.can_block = metadata.can_block;
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
        self.execute(&Self::compose(harness, source, strict), "<test262-harness>")
    }

    fn run_harnessed_script_at(
        &mut self,
        harness: &[&str],
        source: &str,
        strict: bool,
        path: &Path,
    ) -> Result<(), String> {
        self.execute(
            &Self::compose(harness, source, strict),
            &path.display().to_string(),
        )
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
