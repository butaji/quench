//! Shared-VM entry execution used by the development CLI and inventory worker.

use crate::host::NodeHost;
use quench_runtime::{ExecutionRequest, MemoryPolicy, Runtime, SourceKind};
use std::{path::PathBuf, process::ExitCode};

/// Initialize signal policy only at an executable's process boundary. Embedders
/// using execute_shared retain ownership of their inherited signal dispositions.
#[cfg(target_os = "linux")]
pub fn initialize_cli_process_signals() -> std::io::Result<()> {
    crate::modules::process_signals_state::reset_cli_dispositions()
}

const UNSETTLED_TOP_LEVEL_AWAIT_EXIT: u8 = 13;
/// Select the child process's raw stdout/stderr boundary for Quench workers.
pub const CHILD_RUNNER_ENV: &str = "QUENCH_CHILD_RUNNER";
/// Optional installation root containing Node's lib/ and deps/ source assets.
pub const NODE_LIBRARY_ROOT_ENV: &str = "QUENCH_NODE_LIBRARY_ROOT";
/// Names the process's memory policy (`MemoryPolicy::name`); unset, it is the default
/// (throughput). Child processes inherit it with the environment.
pub const MEMORY_POLICY_ENV: &str = "QUENCH_MEMORY_POLICY";
/// The CLI's form of `MEMORY_POLICY_ENV`, which takes precedence over it.
const MEMORY_POLICY_FLAG: &str = "--memory-policy";

/// The options that name the inline source (`-e`, `-p` and their combined forms), each
/// followed by the source; a print option followed by another option is a switch instead.
const INLINE_SOURCE_OPTIONS: &[&str] = &["-e", "--eval", "-pe", "-ep", "-p", "--print"];
const PRINT_SWITCHES: &[&str] = &["-p", "--print"];
/// The forms that carry the inline source after an equals sign.
const INLINE_SOURCE_ASSIGNMENTS: &[&str] = &["--eval=", "--print="];

/// Whether `exec_argv` holds only the inline source's options and their sources, as the CLI
/// records them (none at all included): the bootstrap reads no other option.
pub(crate) fn names_only_inline_source(exec_argv: &[String]) -> bool {
    let mut entries = exec_argv.iter().peekable();
    while let Some(entry) = entries.next() {
        if INLINE_SOURCE_ASSIGNMENTS.iter().any(|form| entry.starts_with(form)) {
            continue;
        }
        if !INLINE_SOURCE_OPTIONS.contains(&entry.as_str()) {
            return false;
        }
        let switch = PRINT_SWITCHES.contains(&entry.as_str())
            && entries.peek().is_none_or(|next| next.starts_with('-'));
        if !switch {
            entries.next();
        }
    }
    true
}

/// Under the throughput policy (`MemoryPolicy::Throughput`), glibc's trim threshold is raised
/// from its 128 KiB default, which pinning the mmap threshold (quench-node's `main`) also pins:
/// once that much is free at the top of an arena it is returned (`MADV_DONTNEED` in a thread's
/// arena) and faulted back in by the next allocations. A server freeing and reallocating
/// stream chunks then spent its time in page faults (video route, 10 connections, 2026-10-22:
/// ~600K minor faults per 6 s; express 38 -> 58 and fastify 36 -> 67 requests/s with this
/// threshold, peak VmHWM within 1-4 MB). Freed memory below it stays in the arena for reuse.
/// The memory-first policy keeps the default, which returns it.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const TRIM_THRESHOLD_BYTES: libc::c_int = 16 * 1024 * 1024;
/// The glibc variable that sets the trim threshold instead.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const TRIM_THRESHOLD_VARIABLE: &str = "MALLOC_TRIM_THRESHOLD_";

/// The allocator setting the process's `policy` takes, applied once when it is selected
/// (`select_memory_policy`); an explicit `TRIM_THRESHOLD_VARIABLE` wins.
///
/// Throughput serves small requests from Quench's size-classed allocator, which the runtime's
/// bulk frees and reallocations of the same sizes favor: wall time on npm scenarios -4 to -15%
/// (x86_64 Linux VM, 4 cores, 2026-10-20: chalk 125 -> 107 ms, typescript 576 -> 505), and
/// JetStream bigint-noble 8.28 -> 13.87, json-parse-inspector 109.8 -> 133.1 (TOD-167).
/// Memory-first keeps the system allocator: the size classes' partly used pages raised V8-v7
/// max RSS (static x86_64, unbolted, 2026-10-18, KiB): Crypto 4272 -> 4640, Richards 3716 ->
/// 4064, RayTrace 3884 -> 4128, DeltaBlue 3840 -> 4120, RegExp 7612 -> 7980. Blocks the system
/// allocator handed out before the selection stay its own (`quench_alloc::owns`).
fn apply_allocator_policy(policy: MemoryPolicy) {
    #[cfg(target_os = "linux")]
    if policy == MemoryPolicy::Throughput {
        quench_alloc::QuenchAlloc::enable();
    }
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    if policy == MemoryPolicy::Throughput && std::env::var_os(TRIM_THRESHOLD_VARIABLE).is_none() {
        // SAFETY: mallopt only adjusts allocator parameters.
        unsafe {
            libc::mallopt(libc::M_TRIM_THRESHOLD, TRIM_THRESHOLD_BYTES);
        }
    }
    #[cfg(not(all(target_os = "linux", target_env = "gnu")))]
    let _ = policy;
}

/// The process's memory policy. Every runtime the process starts (its entry's, a restore's
/// replacement, its Workers') uses it: the pages a policy returns at collections
/// (`resident_pages`) are the process's, not one runtime's. The first selection holds.
static PROCESS_MEMORY_POLICY: std::sync::OnceLock<MemoryPolicy> = std::sync::OnceLock::new();

/// The policy this process selected, once `select_memory_policy` has run.
pub(crate) fn process_memory_policy() -> Option<MemoryPolicy> {
    PROCESS_MEMORY_POLICY.get().copied()
}

/// Selects the process's memory policy once: the CLI's `chosen` name, else
/// `MEMORY_POLICY_ENV`, else the default. An unknown name is an error.
fn select_memory_policy(chosen: Option<&str>) -> Result<MemoryPolicy, String> {
    if let Some(policy) = PROCESS_MEMORY_POLICY.get() {
        return Ok(*policy);
    }
    let environment = std::env::var_os(MEMORY_POLICY_ENV);
    let (source, name) = match (chosen, &environment) {
        (Some(name), _) => (MEMORY_POLICY_FLAG, name),
        (None, Some(name)) => (
            MEMORY_POLICY_ENV,
            name.to_str().ok_or_else(|| format!("{MEMORY_POLICY_ENV} is not valid Unicode"))?,
        ),
        (None, None) => return Ok(*PROCESS_MEMORY_POLICY.get_or_init(|| {
            let policy = MemoryPolicy::default();
            apply_allocator_policy(policy);
            policy
        })),
    };
    let policy = MemoryPolicy::from_name(name).ok_or_else(|| {
        format!("{source}: unknown memory policy '{name}' (expected {})", memory_policy_names())
    })?;
    Ok(*PROCESS_MEMORY_POLICY.get_or_init(|| {
        apply_allocator_policy(policy);
        policy
    }))
}

fn memory_policy_names() -> String {
    MemoryPolicy::ALL.map(MemoryPolicy::name).join("|")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryGoal {
    Node,
    CommonJs,
    Module,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharedCompletion {
    Completed,
    PendingTopLevelAwait,
    GuestExit { code: i32 },
}

impl SharedCompletion {
    fn exit_code(self) -> ExitCode {
        match self {
            Self::Completed => ExitCode::SUCCESS,
            Self::PendingTopLevelAwait => ExitCode::from(UNSETTLED_TOP_LEVEL_AWAIT_EXIT),
            Self::GuestExit { code } => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        }
    }
}

/// One source form accepted by the shared Node execution path.
pub enum SharedInput {
    Eval {
        source: String,
        goal: EntryGoal,
        exec_argv: Vec<String>,
        print: bool,
    },
    Stdin {
        source: String,
        goal: EntryGoal,
        exec_argv: Vec<String>,
    },
    File {
        path: PathBuf,
        exec_argv: Vec<String>,
        goal: EntryGoal,
    },
}

/// Execute one entry through the shared VM and the canonical `NodeHost`.
pub fn execute_shared(input: SharedInput, argv: Vec<String>) -> Result<SharedCompletion, String> {
    execute_shared_with_execution_mode(input, argv, quench_runtime::ExecutionMode::Interpreted)
}

/// Execute through the shared VM with the selected interpreter/JIT tier mode.
/// The mode belongs to this Runtime; it does not create a second VM or frontend.
pub fn execute_shared_with_execution_mode(
    input: SharedInput,
    argv: Vec<String>,
    mode: quench_runtime::ExecutionMode,
) -> Result<SharedCompletion, String> {
    execute_shared_with_teardown(input, argv, mode, Teardown::Free)
}

/// What becomes of an entry's runtime once the entry finishes.
#[derive(Clone, Copy)]
enum Teardown {
    /// Dropped: an embedding or a Worker's thread goes on after the entry.
    Free,
    /// Ended for the exit that follows (`Runtime::end_for_process_exit`): the CLI's process
    /// exits once its entry finishes.
    ProcessExit,
}

/// An entry's runtime, ended by its `Teardown` on every path out of the entry.
struct EntryRuntime {
    runtime: std::mem::ManuallyDrop<Runtime<NodeHost>>,
    teardown: Teardown,
}

impl std::ops::Deref for EntryRuntime {
    type Target = Runtime<NodeHost>;
    fn deref(&self) -> &Self::Target {
        &self.runtime
    }
}

impl std::ops::DerefMut for EntryRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.runtime
    }
}

impl Drop for EntryRuntime {
    fn drop(&mut self) {
        // SAFETY: `runtime` is taken once, here, and never used again.
        let runtime = unsafe { std::mem::ManuallyDrop::take(&mut self.runtime) };
        match self.teardown {
            Teardown::Free => drop(runtime),
            Teardown::ProcessExit => runtime.end_for_process_exit(),
        }
    }
}

fn execute_shared_with_teardown(
    input: SharedInput,
    argv: Vec<String>,
    mode: quench_runtime::ExecutionMode,
    teardown: Teardown,
) -> Result<SharedCompletion, String> {
    #[cfg(target_os = "linux")]
    let (_parent_mask, worker_mask) =
        crate::modules::process_signals_state::ParentSignalMask::block_for_worker()
            .map_err(|error| format!("runtime worker signal mask: {error}"))?;
    std::thread::Builder::new()
        .name("quench-node-shared-exec".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(move || {
            #[cfg(target_os = "linux")]
            worker_mask
                .restore()
                .map_err(|error| format!("runtime execution signal mask: {error}"))?;
            execute_shared_on_worker(input, argv, None, mode, teardown)
        })
        .map_err(|error| format!("runtime worker thread: {error}"))?
        .join()
        .unwrap_or_else(|_| Err("runtime worker panicked".into()))
}

/// Run a `worker_threads` Worker's entry on the calling (worker) thread.
pub(crate) fn execute_worker(
    input: SharedInput,
    argv: Vec<String>,
    worker: crate::modules::worker_shared_vm::WorkerContext,
) -> Result<SharedCompletion, String> {
    execute_shared_on_worker(
        input,
        argv,
        Some(worker),
        quench_runtime::ExecutionMode::Interpreted,
        Teardown::Free,
    )
}

fn execute_shared_on_worker(
    input: SharedInput,
    argv: Vec<String>,
    worker: Option<crate::modules::worker_shared_vm::WorkerContext>,
    mode: quench_runtime::ExecutionMode,
    teardown: Teardown,
) -> Result<SharedCompletion, String> {
    let eval_builtin_globals = matches!(&input, SharedInput::Eval { .. });
    let eval_source_nonempty = matches!(&input, SharedInput::Eval { source, .. } if !source.is_empty());
    let inline_source = match &input {
        SharedInput::Stdin { .. } => crate::host::node_host::InlineSource::Stdin,
        _ => crate::host::node_host::InlineSource::Eval,
    };
    let (source, name, kind, commonjs_entry, exec_argv, entry_goal, entry_file) = match input {
        SharedInput::Eval {
            source,
            goal,
            exec_argv,
            print: false,
        }
        | SharedInput::Stdin {
            source,
            goal,
            exec_argv,
        } => (
            source,
            inline_source.compilation_name().to_owned(),
            match goal {
                EntryGoal::Module => SourceKind::Module,
                EntryGoal::Node | EntryGoal::CommonJs => SourceKind::Script,
            },
            None,
            exec_argv,
            goal,
            None,
        ),
        SharedInput::Eval { source, exec_argv, print: true, .. } => {
            let literal = serde_json::to_string(&source).map_err(|error| error.to_string())?;
            (
                format!("console.log(globalThis[\"\\0quench:evalScript\"]({literal}));"),
                inline_source.compilation_name().to_owned(),
                SourceKind::Script,
                None,
                exec_argv,
                EntryGoal::Node,
                None,
            )
        }
        SharedInput::File {
            path,
            exec_argv,
            goal,
        } => {
            let path = crate::host::shared_vm::commonjs::entry_path(&path)?;
            let kind = match goal {
                EntryGoal::Node => NodeHost::source_kind(&path)?,
                EntryGoal::CommonJs => SourceKind::Script,
                EntryGoal::Module => SourceKind::Module,
            };
            let is_module = kind == SourceKind::Module;
            let wasm = path.extension().is_some_and(|extension| extension == "wasm");
            let source = if is_module && wasm {
                crate::host::shared_vm::esm_wasm::entry_facade(&path)?
            } else if is_module {
                std::fs::read_to_string(&path).map_err(|error| error.to_string())?
            } else {
                String::new()
            };
            let name = path.to_string_lossy().into_owned();
            let commonjs_entry = if is_module { None } else { Some(path.clone()) };
            (source, name, kind, commonjs_entry, exec_argv, goal, Some(path))
        }
    };

    let memory_policy = select_memory_policy(None)?;
    let new_host = |worker: Option<crate::modules::worker_shared_vm::WorkerContext>| -> Result<NodeHost, String> {
        let mut host = NodeHost::new(argv.clone()).with_exec_argv(exec_argv.clone())?;
        host.inline_source = inline_source;
        host.eval_builtin_globals = eval_builtin_globals;
        host.eval_source_nonempty = eval_source_nonempty;
        let host = match commonjs_entry.clone() {
            Some(path) => host.with_commonjs_entry_goal(path, entry_goal),
            None => host,
        };
        let shared_state = host.shared_state();
        shared_state.borrow_mut().messaging.context = worker;
        let mut state = shared_state.borrow_mut();
        let argv = state.process_argv.clone();
        let exec_argv = state.exec_argv.clone();
        state.trace.open_environment(argv.as_slice(), &exec_argv);
        drop(state);
        Ok(host)
    };
    let request = ExecutionRequest { source: &source, name: &name, kind };
    let in_worker = worker.is_some();
    let host = new_host(worker)?;
    let snapshot_entry = match &entry_file {
        Some(path) => Some(crate::host::startup_snapshot::SnapshotEntry::File(path)),
        None if eval_builtin_globals => Some(crate::host::startup_snapshot::SnapshotEntry::Inline(&source)),
        None => None,
    };
    let snapshot_path = crate::host::startup_snapshot::snapshot_path(&host, snapshot_entry, in_worker);
    let snapshot = snapshot_path.as_deref().and_then(crate::host::startup_snapshot::load_or_schedule);
    let mut runtime = EntryRuntime {
        runtime: std::mem::ManuallyDrop::new(Runtime::new(host)),
        teardown,
    };
    runtime.set_execution_mode(mode);
    runtime.set_memory_policy(memory_policy);
    let mut program = runtime.compile_entry(request).map_err(|error| format!("{error:?}"))?;
    let mut restored = false;
    if let Some(snapshot) = snapshot {
        match runtime.restore_snapshot(snapshot, &program, |host, reader| host.read_snapshot(reader)) {
            Ok(entry) => {
                crate::host::startup_snapshot::report(snapshot_path.as_deref(), Ok(()));
                program = entry;
                restored = true;
            }
            Err(error) => {
                crate::host::startup_snapshot::report(snapshot_path.as_deref(), Err(&error));
                // The restore may have left the runtime half replaced: boot a fresh one.
                *runtime = Runtime::new(new_host(None)?);
                runtime.set_execution_mode(mode);
                runtime.set_memory_policy(memory_policy);
                program = runtime.compile_entry(request).map_err(|error| format!("{error:?}"))?;
            }
        }
    }
    let shared_state = runtime.host_mut().shared_state();
    if let Some(worker) = &shared_state.borrow().messaging.context {
        worker.control.set_online();
    }
    let execution = if restored {
        runtime.execute_restored(&program)
    } else {
        runtime.execute_deferred_jobs(&program)
    };
    let completion = match execution {
        Ok(()) => crate::modules::process_shared_vm::finish_execution(&mut runtime, &program),
        Err(error) => {
            if shared_state
                .borrow()
                .process_control
                .requested_exit_code()
                .is_some()
            {
                crate::modules::process_shared_vm::finish_requested_exit(&mut runtime, &program)
            } else {
                let exception = error.thrown_value().map(|value| runtime.root(value));
                let mut failure = quench_runtime::RootedError { error, exception };
                let exit = crate::modules::process_shared_vm::finish_after_uncaught_error(
                    &mut runtime,
                    &program,
                    &failure.error,
                );
                let outcome = match exit {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(crate::modules::process_shared_vm::format_uncaught_error(
                        &mut runtime,
                        &program,
                        &mut failure,
                    )),
                    Err(exit_error) => Err(exit_error),
                };
                if let Some(exception) = exception {
                    runtime.release_root(exception);
                }
                outcome
            }
        }
    };
    let reporting = if shared_state
        .borrow()
        .process_control
        .requested_exit_code()
        .is_some()
    {
        Ok(())
    } else {
        runtime
            .finish_deferred_execution(&program)
            .map_err(|error| error.to_string())
    };
    if let Err(error) = completion {
        // A fatal exception sets status 1 before emitting exit. The guest's
        // exit listeners may replace that status; retain the fatal diagnostic
        // while honoring the final status at the CLI/embedding boundary.
        let code = shared_state.borrow().process_control.exit_code();
        if let Some(code) = code.filter(|code| *code != 1) {
            eprintln!("{error}");
            return Ok(SharedCompletion::GuestExit { code });
        }
        return Err(error);
    }
    reporting?;
    // Teardown's drops run code the program never touched; the run's code pages are no
    // longer needed, so only teardown's own become resident beside its memory.
    crate::host::resident_pages::release_read_only_pages();
    if let Some(code) = shared_state.borrow().process_control.exit_code() {
        return Ok(SharedCompletion::GuestExit { code });
    }
    match runtime
        .module_evaluation_pending(&program)
        .map_err(|error| error.to_string())?
    {
        true => Ok(SharedCompletion::PendingTopLevelAwait),
        false => Ok(SharedCompletion::Completed),
    }
}

/// Resolve the Node executable used by embeddings and guest child launches.
/// A colocated CLI handles Node arguments; otherwise the embedding owns that entry.
pub fn executable_path() -> std::io::Result<PathBuf> {
    let executable = std::env::current_exe()?;
    Ok(executable
        .parent()
        .map(|parent| {
            parent.join(if cfg!(windows) {
                "quench-node.exe"
            } else {
                "quench-node"
            })
        })
        .filter(|path| path.is_file())
        .unwrap_or(executable))
}

/// Parse the development CLI's entry form and route it through `execute_shared`. The
/// caller's process exits once this returns, so the entry's runtime is ended for that exit.
pub fn run_shared_cli(arguments: impl IntoIterator<Item = String>) -> Result<ExitCode, String> {
    // A process started to build a startup snapshot builds it instead (`startup_snapshot`);
    // its bootstrap cannot read the environment, so the variable stays out of the snapshot.
    if let Some(path) = std::env::var_os(crate::host::startup_snapshot::BUILD_ENV) {
        return crate::host::startup_snapshot::build(std::path::Path::new(&path)).map(|()| ExitCode::SUCCESS);
    }
    crate::host::resident_pages::map_touched_pages_only();
    let args = arguments.into_iter().collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("--help") | Some("-h") => {
            println!("quench-node [--jit] [{MEMORY_POLICY_FLAG}={}] [-e CODE|SCRIPT]", memory_policy_names());
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }
    let mut exec_argv = Vec::new();
    let mut input_type_module = false;
    let mut input = None;
    let mut eval_source = None;
    let mut print = false;
    let mut jit = false;
    let mut memory_policy = None;
    let mut guest_arguments = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--jit" => jit = true,
            option if option.split_once('=').is_some_and(|(name, _)| name == MEMORY_POLICY_FLAG) => {
                memory_policy = option.split_once('=').map(|(_, value)| value.to_owned());
            }
            "--version" | "-v" => {
                println!("v{}", crate::modules::process_state::NODE_VERSION);
                return Ok(ExitCode::SUCCESS);
            }
            "--input-type=module" => {
                input_type_module = true;
                exec_argv.push(args[index].clone());
            }
            "--input-type=commonjs" => {
                input_type_module = false;
                exec_argv.push(args[index].clone());
            }
            "--experimental-vfs" | "--no-experimental-vfs" => {
                exec_argv.push(args[index].clone());
            }
            option if crate::modules::cli_options::takes_string_value(option) => {
                let option = args[index].clone();
                index += 1;
                let value = args.get(index).cloned().ok_or_else(|| format!("{option} requires an argument"))?;
                exec_argv.extend([option, value]);
            }
            "--trace-event-categories" | "--trace-event-file-pattern" | "-r" | "--require" => {
                let option = args[index].clone();
                index += 1;
                let value = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| format!("{option} requires an argument"))?;
                exec_argv.extend([option, value]);
            }
            "--" => {
                index += 1;
                if eval_source.is_some() || index == args.len() {
                    guest_arguments.extend_from_slice(&args[index..]);
                    break;
                }
                let path = std::env::current_dir()
                    .map_err(|error| error.to_string())?
                    .join(&args[index]);
                guest_arguments.push(path.to_string_lossy().into_owned());
                guest_arguments.extend_from_slice(&args[index + 1..]);
                input = Some(SharedInput::File {
                    path,
                    exec_argv: exec_argv.clone(),
                    goal: EntryGoal::Node,
                });
                break;
            }
            option if INLINE_SOURCE_OPTIONS.contains(&option) => {
                let option = args[index].clone();
                print |= matches!(option.as_str(), "-pe" | "-ep" | "-p" | "--print");
                exec_argv.push(option.clone());
                // Node's optional print argument treats an empty argument as
                // the first guest argv entry, unlike --eval's required argument.
                if PRINT_SWITCHES.contains(&option.as_str())
                    && args.get(index + 1).is_some_and(String::is_empty)
                {
                    eval_source = Some(String::new());
                    guest_arguments.extend_from_slice(&args[index + 1..]);
                    break;
                }
                // --print is also a switch when followed by --eval.
                if PRINT_SWITCHES.contains(&option.as_str())
                    && args.get(index + 1).is_some_and(|next| next.starts_with('-'))
                {
                    index += 1;
                    continue;
                }
                index += 1;
                let source = args.get(index).cloned()
                    .ok_or_else(|| format!("{option} requires an argument"))?;
                exec_argv.push(source.clone());
                eval_source = Some(source);
            }
            option if INLINE_SOURCE_ASSIGNMENTS.iter().any(|form| option.starts_with(form)) => {
                let (name, source) = option.split_once('=').expect("matched equals option");
                print |= name == "--print";
                exec_argv.push(option.to_owned());
                eval_source = Some(source.to_owned());
            }
            _ if eval_source.is_some() && !args[index].starts_with('-') => {
                guest_arguments.extend_from_slice(&args[index..]);
                break;
            }
            "-" => {
                guest_arguments.extend_from_slice(&args[index..]);
                input = Some(SharedInput::Stdin {
                    source: read_cli_stdin()?,
                    goal: if input_type_module {
                        EntryGoal::Module
                    } else {
                        EntryGoal::Node
                    },
                    exec_argv: exec_argv.clone(),
                });
                break;
            }
            option if option.starts_with('-') => {
                exec_argv.push(args[index].clone());
            }
            path => {
                let cwd = if std::path::Path::new(path).is_absolute() {
                    std::path::PathBuf::from(std::path::MAIN_SEPARATOR.to_string())
                } else {
                    std::env::current_dir().map_err(|error| error.to_string())?
                };
                let cwd = cwd.to_string_lossy();
                let paths = [path.to_owned()];
                let path = if cfg!(windows) {
                    crate::modules::path_algorithms::win32::resolve_strings(
                        &paths,
                        &cwd,
                        |device| {
                            std::env::var(format!("={device}")).unwrap_or_else(|_| cwd.to_string())
                        },
                    )
                } else {
                    crate::modules::path_algorithms::posix::resolve_strings(&paths, &cwd)
                };
                guest_arguments.push(path.clone());
                guest_arguments.extend_from_slice(&args[index + 1..]);
                input = Some(SharedInput::File {
                    path: PathBuf::from(path),
                    exec_argv: exec_argv.clone(),
                    goal: EntryGoal::Node,
                });
                break;
            }
        }
        index += 1;
    }
    select_memory_policy(memory_policy.as_deref())?;
    if let Some(source) = eval_source {
        input = Some(SharedInput::Eval {
            source,
            print,
            goal: if input_type_module && !print { EntryGoal::Module } else { EntryGoal::Node },
            exec_argv: exec_argv.clone(),
        });
    }
    if input.is_none() && !crate::modules::tty_state::is_terminal_fd(0) {
        input = Some(SharedInput::Stdin {
            source: read_cli_stdin()?,
            goal: if input_type_module {
                EntryGoal::Module
            } else {
                EntryGoal::Node
            },
            exec_argv: exec_argv.clone(),
        });
    }
    let input = input.unwrap_or_else(|| SharedInput::Eval {
        source: String::new(),
        print: false,
        goal: if input_type_module {
            EntryGoal::Module
        } else {
            EntryGoal::Node
        },
        exec_argv,
    });
    let executable = executable_path().map_err(|error| error.to_string())?;
    let mut argv = vec![executable.to_string_lossy().into_owned()];
    argv.extend(guest_arguments);
    execute_shared_with_teardown(
        input,
        argv,
        if jit {
            quench_runtime::ExecutionMode::Compiled
        } else {
            quench_runtime::ExecutionMode::Interpreted
        },
        Teardown::ProcessExit,
    )
    .map(SharedCompletion::exit_code)
}

fn read_cli_stdin() -> Result<String, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read stdin: {error}"))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// CLI print evaluates a Script and prints its language completion, not a wrapped expression.
pub(crate) fn print_eval_script(context: &mut quench_runtime::NativeContext<'_, NodeHost>, _: quench_runtime::RootId, args: &[quench_runtime::RootId]) -> Result<quench_runtime::RootId, quench_runtime::RootedError> {
    let source = args.first().copied().ok_or_else(|| quench_runtime::RootedError::host("print evaluation source missing"))?;
    let source = context.string_text(source)?.ok_or_else(|| quench_runtime::RootedError::host("print evaluation source must be a string"))?;
    context.evaluate_script_rooted(&source, "[eval]")
}
