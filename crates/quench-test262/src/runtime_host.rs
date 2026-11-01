//! Test262 capability adapter for the canonical shared runtime.

use std::path::Path;

use quench_runtime::{
    CapabilityId, Engine, ExecutionMode, ExecutionRequest, NativeImageSupport, Host, HostFunction, HostFunctionId,
    HostGlobal, ModuleSource, NativeContext, RootId, RootedError, Runtime, SourceKind, SystemHost,
};
use std::time::{Duration, Instant};

use crate::Test262Host;

#[derive(Clone, Copy, Debug)]
pub enum CompilationSite {
    #[cfg(feature = "aot")]
    Harness,
    Entry,
    #[cfg(feature = "aot")]
    Dependency,
}

fn compile_failure(errors: &[quench_runtime::Diagnostic], site: CompilationSite) -> String {
    if !errors.is_empty() && errors.iter().all(quench_runtime::Diagnostic::is_syntax_error) {
        let phase = match site {
            #[cfg(feature = "aot")]
            CompilationSite::Harness => "harness",
            CompilationSite::Entry => "runtime",
            #[cfg(feature = "aot")]
            CompilationSite::Dependency => "resolution",
        };
        format!("quench {phase} SyntaxError: {errors:?}")
    } else {
        format!("quench compilation failed: {errors:?}")
    }
}

#[cfg(feature = "aot")]
#[derive(Debug)]
pub enum NativePreparationError {
    Source { site: CompilationSite, diagnostics: Vec<quench_runtime::Diagnostic> },
    Other(String),
}
#[cfg(feature = "aot")]
impl From<String> for NativePreparationError {
    fn from(message:String)->Self { Self::Other(message) }
}
#[cfg(feature = "aot")]
impl std::fmt::Display for NativePreparationError {
    fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {
        match self {
            Self::Source {site:CompilationSite::Harness,diagnostics}=>write!(f,"quench AOT harness SyntaxError: {diagnostics:?}"),
            Self::Source {site,diagnostics}=>f.write_str(&compile_failure(diagnostics,*site)),
            Self::Other(message)=>f.write_str(message),
        }
    }
}
#[cfg(feature = "aot")]
impl NativePreparationError {
    pub fn expected_syntax_negative(&self,metadata:&crate::TestMetadata)->bool {
        match self {
            Self::Source {site,diagnostics} if !diagnostics.is_empty() && diagnostics.iter().all(quench_runtime::Diagnostic::is_syntax_error)
                && metadata.negative_type.as_deref()==Some("SyntaxError") => match site {
                    CompilationSite::Entry=>matches!(metadata.negative_phase.as_deref(),Some("parse"|"early")),
                    CompilationSite::Dependency=>metadata.negative_phase.as_deref()==Some("resolution"),
                    CompilationSite::Harness=>false,
                },
            _=>false,
        }
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

/// Traverse only declared requests. Compilation neither executes guest code nor
/// guesses dynamic imports; every dependency enters the same atom namespace.
#[cfg(feature = "aot")]
fn compile_native_dependencies(
    host: &mut Test262RuntimeHost,
    compiler: &mut quench_runtime::NativeJsCompiler,
    entry: &quench_runtime::ResidualProgram,
    name: &str,
) -> Result<Vec<(ModuleSource, quench_runtime::ResidualProgram)>, NativePreparationError> {
    use std::collections::{BTreeSet, VecDeque};
    let mut seen = BTreeSet::from([quench_runtime::normalized_module_name(name).into_owned()]);
    let mut pending = VecDeque::from([(name.to_owned(), entry.module_requests().to_vec())]);
    let mut modules = Vec::new();
    while let Some((referrer, requests)) = pending.pop_front() {
        for request in requests {
            if request.phase == quench_runtime::ModuleRequestPhase::Source
                || request.module_type.as_deref().unwrap_or("javascript") != "javascript" { continue; }
            let Some(mut module) = host.resolve_dynamic_import(&referrer, &request.source)
                .map_err(|error| format!("quench AOT dependency resolution: {}", error.message))? else {
                return Err(format!("quench AOT dependency unresolved: {} from {referrer}", request.source).into());
            };
            module.name = quench_runtime::normalized_module_name(&module.name).into_owned();
            if !seen.insert(module.name.clone()) { continue; }
            let program = compiler.compile_module_source(&module).map_err(|diagnostics| NativePreparationError::Source {site:CompilationSite::Dependency,diagnostics})?;
            pending.push_back((module.name.clone(), program.module_requests().to_vec()));
            modules.push((module, program));
        }
    }
    Ok(modules)
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
    native_support: NativeImageSupport,
    native_compilation: crate::NativeCompilation,
    native_variant: &'static str,
    timers: Vec<TestTimer>,
    next_timer_id: u64,
    native_module_sources: std::collections::BTreeMap<String, ModuleSource>,
    module_input_access: ModuleInputAccess,
    missing_module_input: Option<String>,
}

/// One atom namespace shared by exact harness, declared modules, and discovered
/// source requests. Both object linking and in-process verification consume
/// this preparation without executing guest code.
#[cfg(feature = "aot")]
pub struct NativeCasePrograms {
    pub harness: Vec<quench_runtime::ResidualProgram>,
    pub modules: Vec<(ModuleSource, quench_runtime::ResidualProgram)>,
    pub source_requests: std::collections::BTreeMap<quench_runtime::NativeSourceKey, quench_runtime::NativeSourceCompilation>,
    pub entry: quench_runtime::ResidualProgram,
    pub entry_module: Option<ModuleSource>,
}

/// Compiler-bound code and exact module inputs for one standalone test variant.
pub struct LinkedCasePrograms {
    pub harness: Vec<quench_runtime::NativeJsProgram<Test262RuntimeHost>>,
    pub modules: Vec<quench_runtime::NativeJsProgram<Test262RuntimeHost>>,
    pub entry: quench_runtime::NativeJsProgram<Test262RuntimeHost>,
    pub module_sources: Vec<ModuleSource>,
}

#[derive(Debug, Default)]
enum ModuleInputAccess {
    #[default]
    Filesystem,
    Embedded,
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
        let name = quench_runtime::normalized_module_name(&path.to_string_lossy()).into_owned();
        if let Some(module) = self.native_module_sources.get(&name) {
            return Ok(Some(module.clone()));
        }
        if matches!(self.module_input_access, ModuleInputAccess::Embedded) {
            if self.missing_module_input.is_none() { self.missing_module_input = Some(name.clone()); }
            return Err(format!("native image has no module input: {name}").into());
        }
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

    /// Retain the shared interpreter/JIT for source first known at execution.
    /// Static roots, harness and declared dependencies are still precompiled.
    pub fn with_native_dynamic_source(mut self, enabled: bool) -> Result<Self, String> {
        if enabled && (self.execution_mode != ExecutionMode::NativeOnly || !crate::NATIVE_SUPPORT_HAS_JIT) {
            return Err("dynamic native source requires AOT compilation and JIT support".into());
        }
        self.native_support = if enabled { NativeImageSupport::DynamicSource } else { NativeImageSupport::Static };
        self.native_compilation.dynamic_source = self.native_support.allows_dynamic_source();
        Ok(self)
    }

    fn runtime_execution_mode(&self) -> ExecutionMode {
        if self.execution_mode == ExecutionMode::NativeOnly { self.native_support.execution_mode() } else { self.execution_mode }
    }

    fn execute(&mut self, source: &str, name: &str) -> Result<(), String> {
        self.execute_kind(source, name, SourceKind::Script)
    }

    fn execute_kind(&mut self, source: &str, name: &str, kind: SourceKind) -> Result<(), String> {
        self.execute_after_harness(&[], source, name, kind)
    }

    /// Harness files are global Scripts evaluated in the test's realm before
    /// its entry, so modules the entry loads observe the same harness bindings.
    /// Prepare the same static graph for standalone linking and native-only
    /// verification. The caller supplies the runner's exact harness and source
    /// goal; this operation performs no guest execution.
    #[cfg(feature = "aot")]
    pub fn prepare_native_programs(
        &mut self,
        harness: &[&str],
        source: &str,
        name: &str,
        kind: SourceKind,
    ) -> Result<NativeCasePrograms, String> {
        self.prepare_native_programs_typed(harness,source,name,kind).map_err(|error|error.to_string())
    }

    #[cfg(feature = "aot")]
    pub fn prepare_native_programs_typed(
        &mut self,harness:&[&str],source:&str,name:&str,kind:SourceKind,
    )->Result<NativeCasePrograms,NativePreparationError> {
        self.native_module_sources.clear();
        self.module_input_access = ModuleInputAccess::Filesystem;
        self.missing_module_input = None;
        let mut compiler = quench_runtime::NativeJsCompiler::new(false).with_function_source_discovery()
            .with_global_script_source_discovery("$262", "evalScript");
        let mut scripts = Vec::new();
        for script in harness {
            scripts.push(compiler.compile_script(script, HARNESS_SCRIPT_NAME, Default::default())
                .map_err(|diagnostics|NativePreparationError::Source {site:CompilationSite::Harness,diagnostics})?);
        }
        let entry_module = (kind == SourceKind::Module).then(||
            ModuleSource::text(quench_runtime::normalized_module_name(name).into_owned(), source));
        let program = match kind {
            SourceKind::Module => {
                let module = entry_module.as_ref().expect("module source goal");
                self.native_module_sources.insert(module.name.clone(), module.clone());
                compiler.compile_module_source(module)
            }
            SourceKind::Script => compiler.compile_script(source, name, Default::default()),
            SourceKind::Eval => return Err(NativePreparationError::Other("quench AOT: eval source goal is not a static entry".into())),
        }.map_err(|diagnostics|NativePreparationError::Source {site:CompilationSite::Entry,diagnostics})?;
        let modules = compile_native_dependencies(self, &mut compiler, &program, name)?;
        let source_requests = compiler.compile_discovered_sources()
            .map_err(|error| format!("quench AOT source compilation: {error}"))?;
        let entry = compiler.finish(program).map_err(|error| format!("quench AOT namespace: {error}"))?;
        Ok(NativeCasePrograms { harness: scripts, modules, source_requests, entry, entry_module })
    }

    fn execute_after_harness(
        &mut self,
        harness: &[&str],
        source: &str,
        name: &str,
        kind: SourceKind,
    ) -> Result<(), String> {
        self.done = None;
        self.native_module_sources.clear();
        self.module_input_access = ModuleInputAccess::Filesystem;
        self.missing_module_input = None;
        let execution_mode = self.execution_mode;
        let mut runtime = Runtime::new(std::mem::take(self));
        let runtime_mode = runtime.host_mut().runtime_execution_mode();
        runtime.set_execution_mode(runtime_mode);
        let result = (|| {
            #[cfg(feature = "aot")]
            let prepared = if execution_mode == ExecutionMode::NativeOnly {
                Some(runtime.host_mut().prepare_native_programs(harness, source, name, kind)?)
            } else { None };
            let program = if execution_mode == ExecutionMode::NativeOnly {
                #[cfg(feature = "aot")]
                { prepared.as_ref().expect("native preparation").entry.clone() }
                #[cfg(not(feature = "aot"))]
                { return Err("quench AOT code generation requires the aot feature".into()); }
            } else { match kind {
                SourceKind::Script => Engine::specialize_unspecialized(source, name),
                SourceKind::Module => Engine::specialize_module_unspecialized(source, name),
                SourceKind::Eval => Engine::compile(ExecutionRequest { source, name, kind }),
            }
            .map_err(|errors| compile_failure(&errors, CompilationSite::Entry))? };
            let harness = harness
                .iter()
                .map(|script| (*script, HARNESS_SCRIPT_NAME))
                .collect::<Vec<_>>();
            let execution = if execution_mode == ExecutionMode::NativeOnly {
                #[cfg(feature = "aot")]
                {
                    let NativeCasePrograms { harness: native_harness, modules: native_dependencies,
                        source_requests: native_sources, .. } = prepared.expect("native preparation");
                    let host = runtime.host_mut();
                    let output = native_output_directory(name, host.native_variant)?;
                    let mut scripts = Vec::new();
                    for script in native_harness {
                        scripts.push(compile_native_unit(script, &mut host.native_compilation, output.as_deref())?);
                    }
                    let mut modules = Vec::new();
                    for (source, program) in native_dependencies {
                        modules.push(compile_native_unit(program, &mut host.native_compilation, output.as_deref())?);
                        host.native_module_sources.insert(source.name.clone(), source);
                    }
                    let mut catalog = std::collections::BTreeMap::new();
                    for (key, compiled) in native_sources {
                        let artifact = match compiled {
                            quench_runtime::NativeSourceCompilation::Program(program) => Ok(compile_native_unit(program, &mut host.native_compilation, output.as_deref())?),
                            quench_runtime::NativeSourceCompilation::SyntaxError(message) => Err(quench_runtime::NativeSourceError::Syntax(message)),
                        };
                        catalog.insert(key, artifact);
                    }
                    let native = compile_native_unit(program.clone(), &mut host.native_compilation, output.as_deref())?;
                    if !catalog.is_empty() {
                        runtime.set_native_program_resolver(move |name, parts| {
                            let key = quench_runtime::NativeSourceKey::new(name, parts)?;
                            catalog.get(&key).cloned().transpose()
                        });
                    }
                    runtime.execute_native_module_graph(&scripts, &modules, &native)
                }
                #[cfg(not(feature = "aot"))]
                { unreachable!("AOT feature checked before compilation") }
            } else { runtime.execute_after_scripts(&harness, &program) };
            execution
                .map_err(|error| {
                    format!("quench runtime: {}", runtime.format_error(&program, &error))
                })?;
            Self::drain_completion(&mut runtime, &program)
        })();
        self.finish_runtime(runtime, result)
    }

    fn drain_completion(runtime: &mut Runtime<Self>, program: &quench_runtime::ResidualProgram) -> Result<(), String> {
            if runtime.host_mut().async_test {
                loop {
                    runtime.run_jobs(program).map_err(|error| {
                        format!(
                            "quench runtime jobs: {}",
                            runtime.format_error(program, &error)
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
            if program.is_module() && runtime.module_evaluation_pending(program)
                .map_err(|error| format!("quench runtime module: {error}"))? {
                return Err("quench runtime module: evaluation did not settle".into());
            }
        Ok(())
    }

    fn finish_runtime(&mut self, mut runtime: Runtime<Self>, result: Result<(), String>) -> Result<(), String> {
        for timer in runtime.host_mut().take_all_timers() {
            runtime.release_root(timer.callback);
            for arg in timer.args { runtime.release_root(arg); }
        }
        let missing = runtime.host_mut().missing_module_input.take();
        let rejections = runtime.native_execution_rejections().saturating_add(u64::from(missing.is_some()));
        runtime.host_mut().native_compilation.rejections = rejections;
        *self = runtime.into_host();
        self.native_module_sources.clear();
        self.module_input_access = ModuleInputAccess::Filesystem;
        if rejections != 0 {
            Err(format!("quench AOT capability rejection: {rejections} native code requests unavailable; outcome: {result:?}{}", missing.map(|name|format!("; missing module: {name}")).unwrap_or_default()))
        } else { result }
    }

    /// Execute compiler-bound objects through the same harness/job/completion
    /// contract as source-based verification. The host's execution mode selects
    /// strict native support or linked code plus the shared interpreter/JIT.
    /// Module inputs remain explicit in both modes; no filesystem fallback.
    pub fn execute_linked_programs(
        &mut self,
        graph: LinkedCasePrograms,
        resolver: impl Fn(&str, &[&str]) -> Result<Option<quench_runtime::NativeJsProgram<Self>>, quench_runtime::NativeSourceError> + 'static,
    ) -> Result<(), String> {
        self.execute_linked_programs_resolved(graph,Box::new(resolver))
    }

    // One SDK execution body serves every catalog. The resolver's concrete
    // closure type does not specialize VM construction or the completion loop.
    #[inline(never)]
    fn execute_linked_programs_resolved(
        &mut self, graph:LinkedCasePrograms, resolver:Box<quench_runtime::NativeProgramResolver<Self>>,
    ) -> Result<(),String> {
        self.done = None;
        let execution_mode = self.runtime_execution_mode();
        self.native_module_sources = graph.module_sources.into_iter().map(|mut source| {
            source.name = quench_runtime::normalized_module_name(&source.name).into_owned();
            (source.name.clone(), source)
        }).collect();
        self.module_input_access = ModuleInputAccess::Embedded;
        self.missing_module_input = None;
        let mut runtime = Runtime::new(std::mem::take(self));
        runtime.set_execution_mode(execution_mode);
        runtime.set_native_program_resolver_boxed(resolver);
        let program = graph.entry.residual();
        let result = runtime.execute_native_module_graph(&graph.harness, &graph.modules, &graph.entry)
            .map_err(|error| format!("quench runtime: {}", runtime.format_error(program, &error)))
            .and_then(|_| Self::drain_completion(&mut runtime, program));
        self.finish_runtime(runtime, result)
    }

    fn execute_harnessed_script(
        &mut self, harness: &[&str], source: &str, strict: bool, name: &str,
    ) -> Result<(), String> {
        let source = crate::script_source(source, strict);
        self.execute_after_harness(harness, &source, name, SourceKind::Script)
    }

}

impl Test262Host for Test262RuntimeHost {
    fn configure(&mut self, metadata: &crate::TestMetadata) {
        self.async_test = metadata.is_async;
        self.can_block = metadata.can_block;
        self.native_compilation = crate::NativeCompilation { dynamic_source: self.native_support.allows_dynamic_source(), ..Default::default() };
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

    #[cfg(feature = "aot")]
    #[test]
    fn linked_graph_shares_dynamic_source_semantics_and_resets_after_rejection() {
        use quench_runtime::{ExecutionMode, NativeJsProgram, NativeSourceError, SourceKind};
        // Inputs become known only inside guest execution. Native roots call
        // dynamic bodies through the same heap, lexical environment and jobs.
        let source = r#"
            (function() {
            const local = 40;
            const text = ['local', '+', '2'].join('');
            if (eval(text) !== 42) throw Error('direct eval lexical environment');
            const FunctionKind = (function() {}).constructor;
            const body = ['return', 'x + 2'].join(' ');
            const fn = FunctionKind('x', body);
            if (fn(40) !== 42 || fn === FunctionKind('x', body)) throw Error('Function value or identity');
            Promise.resolve().then(() => {
                if ((0, eval)(['typeof', 'local'].join(' ')) !== 'undefined') throw Error('indirect eval scope');
                $DONE();
            });
            })();
        "#;
        for mode in [ExecutionMode::NativeOnly, ExecutionMode::Compiled] {
            let mut host = Test262RuntimeHost::new(ExecutionMode::NativeOnly)
                .with_native_dynamic_source(mode == ExecutionMode::Compiled).unwrap();
            for _ in 0..2 {
                host.async_test = true;
                let prepared = host.prepare_native_programs(&[], source, "linked-dynamic.js", SourceKind::Script).unwrap();
                let graph = super::LinkedCasePrograms {
                    harness: vec![], modules: vec![],
                    entry: NativeJsProgram::compile_eager(prepared.entry).unwrap(),
                    module_sources: vec![],
                };
                let result = host.execute_linked_programs(graph, |_, _| Ok::<_, NativeSourceError>(None));
                if mode == ExecutionMode::Compiled {
                    result.unwrap();
                    assert_eq!(host.native_compilation.rejections, 0);
                    assert!(host.native_compilation.dynamic_source);
                } else {
                    assert!(result.unwrap_err().starts_with("quench AOT capability rejection:"));
                    assert!(host.native_compilation.rejections > 0);
                }
                assert!(host.native_module_sources.is_empty());
                assert!(host.timers.is_empty());
                assert!(matches!(host.module_input_access, super::ModuleInputAccess::Filesystem));
            }
            if mode == ExecutionMode::Compiled {
                host.execute(source, "hybrid-source-verification.js").unwrap();
                assert!(!host.native_compilation.units.is_empty());
                assert_eq!(host.native_compilation.rejections, 0);
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn strict_dynamic_scripts_preserve_hashbangs_and_captured_global_bindings() {
        let sources = [r#"
            'use strict';
            for (const text of ['#!', '#!comment', '#!\n', '#!\r', '#!\u2028', '#!\u2029']) {
                if (eval(text) !== undefined) throw Error('hashbang completion');
            }
            if (eval('#!comment\n40 + 2') !== 42) throw Error('hashbang body');
            class PrivateContext {
                #value = 42;
                read() { return eval('#!comment\nthis.#value'); }
            }
            if (new PrivateContext().read() !== 42) throw Error('private eval context');
            try { eval(' #!\n'); throw Error('accepted misplaced hashbang'); }
            catch (error) { if (!(error instanceof SyntaxError)) throw error; }
        "#, r#"
            'use strict';
            globalThis.objectBinding = 1;
            Object.preventExtensions(globalThis);
            $262.evalScript('let lexicalBinding = 1; const constantBinding = 3; class classBinding {}');
            lexicalBinding = 2;
            classBinding = 5;
            if (lexicalBinding !== 2 || constantBinding !== 3 || classBinding !== 5) throw Error('lexical values');
            for (const name of ['lexicalBinding', 'constantBinding', 'classBinding']) {
                if (Object.hasOwn(globalThis, name)) throw Error('lexical became property');
            }
            try { constantBinding = 4; throw Error('constant assignment accepted'); }
            catch (error) { if (!(error instanceof TypeError)) throw error; }
            objectBinding = ($262.evalScript('let objectBinding = 100;'), 2);
            if (globalThis.objectBinding !== 1 || objectBinding !== 2) throw Error('global environment reference');
            let trace = '';
            try { $262.evalScript('throw 1; let uninitializedBinding;'); } catch (error) { if (error !== 1) throw error; }
            try { uninitializedBinding = (trace += 'rhs', 2); throw Error('TDZ assignment accepted'); }
            catch (error) { if (!(error instanceof ReferenceError) || trace !== 'rhs') throw error; }
        "#, r#"
            'use strict';
            let callerLexical = 1;
            $262.evalScript('callerLexical = 2;');
            if (callerLexical !== 2) throw Error('caller lexical did not update');
            callerLexical = 3;
            if ($262.evalScript('callerLexical') !== 3) throw Error('caller lexical read stale');
            const callerConstant = 1;
            try { $262.evalScript('callerConstant = 2;'); throw Error('caller constant assigned'); }
            catch (error) { if (!(error instanceof TypeError)) throw error; }
            $262.evalScript('let nestedLexical = 1; $262.evalScript("nestedLexical = 2;");');
            if (nestedLexical !== 2) throw Error('finished script overwrote nested lexical');
        "#, r#"
            'use strict';
            let capturedCaller = 1;
            const readCaller = () => capturedCaller;
            $262.evalScript('capturedCaller = 2;');
            if (readCaller() !== 2) throw Error('active captured caller');
            globalThis.writeCaller = value => capturedCaller = value;
            $262.evalScript('writeCaller(5);');
            capturedCaller = 6;
            if (readCaller() !== 6 || $262.evalScript('capturedCaller') !== 6) throw Error('caller closure write authority');
            $262.evalScript('let retained = 1; globalThis.readRetained = () => retained; globalThis.writeRetained = value => retained = value; $262.evalScript("retained = 2;");');
            if (readRetained() !== 2) throw Error('nested captured caller');
            $262.evalScript('retained = 3;');
            if (readRetained() !== 3) throw Error('retained global lexical closure');
            writeRetained(4);
            if ($262.evalScript('retained') !== 4) throw Error('retained closure write');
        "#];
        for source in sources {
            for mode in [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled, super::ExecutionMode::NativeOnly] {
                let mut host = Test262RuntimeHost::new(mode)
                    .with_native_dynamic_source(mode == super::ExecutionMode::NativeOnly).unwrap();
                host.execute(source, "dynamic-script-contract.js").unwrap_or_else(|error| panic!("{mode:?}: {error}"));
                assert_eq!(host.native_compilation.rejections, 0);
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn global_lexicals_outlive_the_script_root() {
        let source = r#"
            let laterLive = 1;
            const readLater = () => laterLive;
            Promise.resolve().then(() => {
                $262.evalScript('laterLive = 2;');
                if (readLater() !== 2) throw Error('completed root lexical');
                laterLive = 3;
                if ($262.evalScript('laterLive') !== 3) throw Error('completed root closure write');
            }).then($DONE, $DONE);
        "#;
        for mode in [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled, super::ExecutionMode::NativeOnly] {
            let mut host = Test262RuntimeHost::new(mode)
                .with_native_dynamic_source(mode == super::ExecutionMode::NativeOnly).unwrap();
            host.async_test = true;
            host.execute(source, "completed-script-root.js").unwrap_or_else(|error| panic!("{mode:?}: {error}"));
            assert_eq!(host.native_compilation.rejections, 0);
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn async_module_imports_preserve_waiter_order_and_namespace_resolution() {
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("entry.js");
        std::fs::write(directory.path().join("waiting.mjs"),
            "globalThis.started.resolve(); await globalThis.release.promise;").unwrap();
        std::fs::write(directory.path().join("empty.mjs"), "").unwrap();
        std::fs::write(directory.path().join("then.mjs"),
            "await 0; export function then(resolve) { resolve(42); }").unwrap();
        let source = r#"
            globalThis.started = Promise.withResolvers();
            globalThis.release = Promise.withResolvers();
            const trace = [];
            const first = import('./waiting.mjs');
            started.promise.then(async () => {
                const second = import('./waiting.mjs');
                await import('./empty.mjs');
                release.resolve();
                await Promise.all([first.then(() => trace.push('first')), second.then(() => trace.push('second'))]);
                if (trace.join(',') !== 'first,second') throw Error('import waiter order');
                const value = await import('./then.mjs');
                if (value !== 42) throw Error('namespace then was not resolved');
            }).then($DONE, $DONE);
        "#;
        for mode in [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled, super::ExecutionMode::NativeOnly] {
            let mut host = Test262RuntimeHost::new(mode)
                .with_native_dynamic_source(mode == super::ExecutionMode::NativeOnly).unwrap();
            host.async_test = true;
            host.execute(source, entry.to_str().unwrap()).unwrap_or_else(|error| panic!("{mode:?}: {error}"));
            assert_eq!(host.native_compilation.rejections, 0);
        }
    }

    #[test]
    fn strict_destructuring_checks_missing_bindings_after_getters_and_defaults() {
        let source = r#"
            const effects = [];
            (function() {
                'use strict';
                try {
                    0, { missingAssignment } = { get missingAssignment() { effects.push('get'); return 1; } };
                    throw Error('missing ReferenceError');
                } catch (error) { if (!(error instanceof ReferenceError)) throw error; }
                try {
                    ({ missingAssignment = (effects.push('default'), 2) } = {});
                    throw Error('missing ReferenceError');
                } catch (error) { if (!(error instanceof ReferenceError)) throw error; }
                try {
                    ({ missingAssignment } = { get missingAssignment() { effects.push('abrupt'); throw new RangeError('rhs'); } });
                    throw Error('missing getter error');
                } catch (error) { if (!(error instanceof RangeError)) throw error; }
            })();
            if (effects.join(',') !== 'get,default,abrupt') throw Error(effects.join(','));
        "#;
        let modes = [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled].into_iter();
        #[cfg(feature = "aot")]
        let modes = modes.chain([super::ExecutionMode::NativeOnly]);
        for mode in modes { Test262RuntimeHost::new(mode).execute(source, "strict-destructuring.js").unwrap(); }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_function_catalog_covers_harness_modules_nested_bodies_and_resets() {
        use std::fs;
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("entry.mjs");
        fs::write(directory.path().join("dep.mjs"), r#"
            export const fromModule = Function('x', 'return x + 2;');
            export const nested = Function("return Function('return 42;');");
        "#).unwrap();
        let source = r#"
            import { fromModule, nested } from './dep.mjs';
            if (fromHarness() !== 40 || fromModule(40) !== 42 || nested()() !== 42) throw Error('source graph');
            const fresh = Function('return 42;');
            if (fresh === nested() || fresh.prototype === nested().prototype) throw Error('identity');
            if (false) Function('let invalid = ;');
            let effects = '';
            try { Function({toString() { effects += 'body'; return 'let invalid = ;'; }}); throw Error('accepted syntax'); }
            catch (error) { if (!(error instanceof SyntaxError) || effects !== 'body') throw error; }
        "#;
        let modes = [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled, super::ExecutionMode::NativeOnly];
        for mode in modes {
            let mut host = Test262RuntimeHost::new(mode);
            host.execute_after_harness(&["function fromHarness() { return Function('return 40;')(); }"],
                source, entry.to_str().unwrap(), super::SourceKind::Module).unwrap();
            assert_eq!(host.native_compilation.rejections, 0);
            assert!(host.native_module_sources.is_empty());
            if mode == super::ExecutionMode::NativeOnly {
                let error = host.execute("const body = 'return 42;'; Function(body)();", "unknown.js").unwrap_err();
                assert!(error.starts_with("quench AOT capability rejection:"), "{error}");
            }
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_function_catalog_keeps_async_and_generator_kinds() {
        let source = r#"
            const generator = new (function*(){}).constructor('yield 42;');
            const async = (async function(){}).constructor('return 42;');
            const asyncGenerator = (async function*(){}).constructor('yield 42;');
            if (generator().next().value !== 42) throw Error('generator');
            Promise.all([async(), asyncGenerator().next()]).then(values => {
                if (values[0] !== 42 || values[1].value !== 42 || values[1].done) throw Error('async');
                $DONE();
            });
        "#;
        for mode in [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled, super::ExecutionMode::NativeOnly] {
            let mut host = Test262RuntimeHost::new(mode);
            host.async_test = true;
            host.execute(source, "constructor-kinds.js").unwrap();
            assert_eq!(host.native_compilation.rejections, 0);
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn function_constructor_validation_uses_the_combined_parameter_context() {
        let source = r#"
            const first = Function('a/*', '#!comment*/', 'return a;');
            const second = Function('/*', '#!comment*/', 'return 42;');
            if (first(42) !== 42 || first.length !== 1 || second() !== 42 || second.length !== 0) throw Error('comment context');
            if (first.toString() !== 'function anonymous(a/*,#!comment*/\n) {\nreturn a;\n}') throw Error('source text');
            for (const action of [() => Function('a', '#!bad'), () => Function('#!bad', 'return 1;')]) {
                try { action(); throw Error('accepted hashbang'); }
                catch (error) { if (!(error instanceof SyntaxError) || error.message !== 'Invalid or unexpected token') throw error; }
            }
        "#;
        for mode in [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled, super::ExecutionMode::NativeOnly] {
            Test262RuntimeHost::new(mode).execute(source, "parameter-comment-context.js").unwrap();
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn constructor_aliases_keep_scope_assignment_and_loop_identities_guarded() {
        let source = r#"
            const Alias = Function;
            let assigned;
            assigned = Alias;
            const constructors = [assigned, (function*(){}).constructor];
            for (const Target of constructors) {
                const created = new Target('return 42;');
                const value = created();
                if (typeof value === 'object') {
                    const result = value.next();
                    if (result.value !== 42 || !result.done) throw Error('generator alias');
                } else if (value !== 42) throw Error('ordinary alias');
            }
            {
                const Alias = () => 7;
                if (Alias('let invalid = ;') !== 7) throw Error('shadowed alias');
            }
            const Async = (async function(){}).constructor;
            const Generator = (function*(){}).constructor;
            const AsyncGenerator = (async function*(){}).constructor;
            for (const Target of [Function, Async, Generator, AsyncGenerator]) {
                try { Target('#!\n_', ''); throw Error('accepted source'); }
                catch (error) { if (!(error instanceof SyntaxError)) throw error; }
            }
        "#;
        for mode in [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled, super::ExecutionMode::NativeOnly] {
            let mut host = Test262RuntimeHost::new(mode);
            host.execute(source, "constructor-aliases.js").unwrap();
            assert_eq!(host.native_compilation.rejections, 0);
        }
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_regions_keep_exact_loop_and_exception_continuations() {
        let program = super::Engine::specialize_unspecialized(r#"
            function calculate(n = 8) {
                let sum = 0;
                for (let i = 0; i < n; i++) {
                    try { if (i === 3) throw i; sum += i; }
                    catch (error) { sum += 2 * error; }
                    finally { sum += 1; }
                }
                return sum;
            }
            if (calculate() !== 39) throw Error('wrong region continuation');
        "#, "region-continuations.js").unwrap();
        let options = quench_runtime::NativeJsCodegenOptions { region_instruction_budget: std::num::NonZeroUsize::new(4).unwrap(), backend: quench_runtime::NativeJsBackend::Cranelift };
        let module = quench_runtime::lower_native_js_module_with_options(&program, "regions", options).unwrap();
        quench_runtime::emit_native_module("regions", module.functions, module.data).unwrap();
        let native = quench_runtime::NativeJsProgram::compile_eager_with_options(program, options).unwrap();
        let mut runtime = super::Runtime::new(Test262RuntimeHost::new(super::ExecutionMode::NativeOnly));
        runtime.set_execution_mode(super::ExecutionMode::NativeOnly);
        runtime.execute_native(&native).unwrap();
        assert_eq!(runtime.native_execution_rejections(), 0);
    }

    #[test]
    fn strict_call_assignment_is_a_syntax_error_not_a_backend_rejection() {
        let errors = super::Engine::specialize("'use strict'; f() = 1;", "strict-call-assignment.js").unwrap_err();
        assert!(super::compile_failure(&errors, super::CompilationSite::Entry).starts_with("quench runtime SyntaxError:"));
    }

    #[cfg(feature = "aot")]
    #[test]
    fn compiler_negatives_require_matching_type_phase_and_compilation_site() {
        let mut host=Test262RuntimeHost::new(super::ExecutionMode::NativeOnly);
        let error=host.prepare_native_programs_typed(&[],"'use strict'; arguments=7;","negative.js",super::SourceKind::Script)
            .err().expect("strict assignment compiled");
        let mut metadata=crate::TestMetadata {negative_phase:Some("parse".into()),negative_type:Some("SyntaxError".into()),..Default::default()};
        assert!(error.expected_syntax_negative(&metadata));
        metadata.negative_phase=Some("runtime".into());
        assert!(!error.expected_syntax_negative(&metadata));
        metadata.negative_phase=Some("parse".into());metadata.negative_type=Some("TypeError".into());
        assert!(!error.expected_syntax_negative(&metadata));
        metadata.negative_type=Some("SyntaxError".into());
        let harness=host.prepare_native_programs_typed(&["let invalid = ;"],"'use strict'; arguments=7;","negative.js",super::SourceKind::Script)
            .err().expect("invalid harness compiled");
        assert!(!harness.expected_syntax_negative(&metadata));
        assert!(!super::NativePreparationError::Other("SyntaxError: fabricated backend text".into()).expected_syntax_negative(&metadata));
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

    #[cfg(feature = "aot")]
    #[test]
    fn native_static_graph_compiles_cycles_once_and_preserves_harness_await_and_live_bindings() {
        use std::fs;
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("sub")).unwrap();
        let entry = directory.path().join("entry.mjs");
        let source = r#"
            import { count, bump } from './dep.mjs';
            import * as first from './dep.mjs';
            export function seed() { return harnessSeed; }
            if (count !== 41) throw Error('initial binding');
            bump();
            if (count !== 42) throw Error('live binding');
            import('./sub/../dep.mjs').then(second => {
                if (first !== second || second.count !== 42) throw Error('namespace identity');
            });
        "#;
        fs::write(&entry, source).unwrap();
        fs::write(directory.path().join("dep.mjs"), "import { seed } from './sub/../entry.mjs'; export let count = await seed(); export function bump() { count++; }").unwrap();
        let mut host = Test262RuntimeHost::new(super::ExecutionMode::NativeOnly);
        host.execute_after_harness(&["const harnessSeed = 41;"], source, entry.to_str().unwrap(), super::SourceKind::Module).unwrap();
        assert_eq!(host.native_compilation.units.len(), 3);
        assert_eq!(host.native_compilation.rejections, 0);
        assert!(host.native_module_sources.is_empty());
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_static_graph_freezes_sources_before_execution_and_rejects_uncompiled_dynamic_source() {
        use std::fs;
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("entry.mjs");
        let source = "import { value } from './dep.mjs'; globalThis.saved = () => value; if (value !== 42) throw Error('bad dependency');";
        fs::write(directory.path().join("dep.mjs"), "export const value = harnessSeed;").unwrap();
        let mut host = Test262RuntimeHost::new(super::ExecutionMode::NativeOnly);
        let mut compiler = quench_runtime::NativeJsCompiler::new(false);
        let harness = compiler.compile_script("const harnessSeed = 42;", "harness.js", Default::default()).unwrap();
        let program = compiler.compile_module(source, entry.to_str().unwrap()).unwrap();
        let dependencies = super::compile_native_dependencies(&mut host, &mut compiler, &program, entry.to_str().unwrap()).unwrap();
        let program = compiler.finish(program).unwrap();
        fs::remove_file(directory.path().join("dep.mjs")).unwrap();
        let modules = dependencies.into_iter().map(|(source, program)| {
            let code = super::compile_native_unit(program, &mut host.native_compilation, None).unwrap();
            host.native_module_sources.insert(source.name.clone(), source);
            code
        }).collect::<Vec<_>>();
        let harness = super::compile_native_unit(harness, &mut host.native_compilation, None).unwrap();
        let native = super::compile_native_unit(program, &mut host.native_compilation, None).unwrap();
        let mut runtime = super::Runtime::new(host);
        runtime.set_execution_mode(super::ExecutionMode::NativeOnly);
        runtime.execute_native_module_graph(&[harness], &modules, &native).unwrap();
        let program = native.residual().clone();
        drop(modules);
        drop(native);
        runtime.collect(&program).unwrap();
        let global = runtime.global_root().unwrap();
        let key = runtime.string_rooted("saved");
        let saved = runtime.get_property_rooted(global, key).unwrap();
        let receiver = runtime.root(quench_runtime::Value::UNDEFINED);
        let result = runtime.call_rooted(saved, receiver, &[]).unwrap();
        assert_eq!(runtime.rooted_value(result).unwrap().as_number(), Some(42.0));
        assert_eq!(runtime.native_execution_rejections(), 0);
        drop(runtime);
        let mut host = Test262RuntimeHost::new(super::ExecutionMode::NativeOnly);
        fs::write(directory.path().join("dep.mjs"), "export const value = 42;").unwrap();
        host.async_test = true;
        let result = host.execute_kind("import('./dep.mjs').then(() => $DONE(), () => $DONE());", entry.to_str().unwrap(), super::SourceKind::Module);
        assert!(result.unwrap_err().starts_with("quench AOT capability rejection:"));
        assert!(host.native_compilation.rejections > 0);
    }

    #[cfg(feature = "aot")]
    #[test]
    fn native_deferred_graph_links_before_evaluation_and_keeps_nested_bodies_deferred() {
        use std::fs;
        let directory = tempfile::tempdir().unwrap();
        let entry = directory.path().join("entry.mjs");
        let source = r#"
            import defer * as deferred from './dep.mjs';
            if (effects.length !== 0) throw Error('eager deferred evaluation');
            const child = deferred.child;
            if (effects.join(',') !== 'first,dep') throw Error('deferred dependency order');
            if (child.answer !== 42 || effects.join(',') !== 'first,dep,child') throw Error('nested deferred evaluation');
        "#;
        fs::write(&entry, source).unwrap();
        fs::write(directory.path().join("dep.mjs"), "import './first.mjs'; import defer * as child from './child.mjs'; effects.push('dep'); export { child };").unwrap();
        fs::write(directory.path().join("first.mjs"), "effects.push('first');").unwrap();
        fs::write(directory.path().join("child.mjs"), "effects.push('child'); export const answer = 42;").unwrap();
        let modes = [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled, super::ExecutionMode::NativeOnly];
        for mode in modes {
            let mut host = Test262RuntimeHost::new(mode);
            host.execute_after_harness(&["globalThis.effects = [];"], source, entry.to_str().unwrap(), super::SourceKind::Module).unwrap();
            assert_eq!(host.native_compilation.rejections, 0);
        }
    }

    #[test]
    fn unsettled_module_evaluation_is_never_a_conformance_pass() {
        let modes = [super::ExecutionMode::Interpreted, super::ExecutionMode::Compiled].into_iter();
        #[cfg(feature = "aot")]
        let modes = modes.chain([super::ExecutionMode::NativeOnly]);
        for mode in modes {
            let mut host = Test262RuntimeHost::new(mode);
            let result = host.execute_kind("await new Promise(() => {});", "pending.mjs", super::SourceKind::Module);
            assert_eq!(result.unwrap_err(), "quench runtime module: evaluation did not settle");
            host.execute_kind("await Promise.resolve();", "settled.mjs", super::SourceKind::Module).unwrap();
        }
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
