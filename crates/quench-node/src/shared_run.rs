//! Shared-VM entry execution used by the development CLI and inventory worker.

use crate::host::NodeHost;
use quench_runtime::{Engine, ExecutionRequest, Runtime, SourceKind};
use std::{path::PathBuf, process::ExitCode};

const UNSETTLED_TOP_LEVEL_AWAIT_EXIT: u8 = 13;
/// Select the child process's raw stdout/stderr boundary for Quench workers.
pub const CHILD_RUNNER_ENV: &str = "QUENCH_CHILD_RUNNER";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryGoal {
    Node,
    CommonJs,
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
    Eval(String),
    File {
        path: PathBuf,
        exec_argv: Vec<String>,
        goal: EntryGoal,
    },
}

/// Execute one entry through the shared VM and the canonical `NodeHost`.
pub fn execute_shared(input: SharedInput, argv: Vec<String>) -> Result<SharedCompletion, String> {
    std::thread::Builder::new()
        .name("quench-node-shared-exec".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(move || execute_shared_on_worker(input, argv))
        .map_err(|error| format!("runtime worker thread: {error}"))?
        .join()
        .unwrap_or_else(|_| Err("runtime worker panicked".into()))
}

fn execute_shared_on_worker(
    input: SharedInput,
    argv: Vec<String>,
) -> Result<SharedCompletion, String> {
    let (source, name, kind, commonjs_entry, exec_argv, entry_goal) = match input {
        SharedInput::Eval(source) => (
            source,
            "<eval>".to_owned(),
            SourceKind::Script,
            None,
            Vec::new(),
            EntryGoal::Node,
        ),
        SharedInput::File {
            path,
            exec_argv,
            goal,
        } => {
            let kind = match goal {
                EntryGoal::Node => NodeHost::source_kind(&path)?,
                EntryGoal::CommonJs => SourceKind::Script,
            };
            let is_module = kind == SourceKind::Module;
            let source = if is_module {
                std::fs::read_to_string(&path).map_err(|error| error.to_string())?
            } else {
                String::new()
            };
            let name = path.to_string_lossy().into_owned();
            let commonjs_entry = if is_module { None } else { Some(path) };
            (source, name, kind, commonjs_entry, exec_argv, goal)
        }
    };

    let host = NodeHost::new(argv).with_exec_argv(exec_argv)?;
    let host = match commonjs_entry {
        Some(path) => host.with_commonjs_entry_goal(path, entry_goal),
        None => host,
    };
    let shared_state = host.shared_state();
    let mut runtime = Runtime::new(host);
    let program = Engine::compile(ExecutionRequest {
        source: &source,
        name: &name,
        kind,
    })
    .map_err(|error| format!("{error:?}"))?;
    let execution = runtime.execute_deferred_jobs(&program);
    let completion = match execution {
        Ok(()) => crate::modules::process_shared_vm::finish_execution(&mut runtime, &program),
        Err(error) => {
            if shared_state
                .borrow()
                .process_control
                .requested_exit_code()
                .is_some()
            {
                Ok(())
            } else {
                let message = runtime.format_error(&program, &error);
                let exit = crate::modules::process_shared_vm::finish_after_uncaught_error(
                    &mut runtime,
                    &program,
                    &error,
                );
                match exit {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(message),
                    Err(exit_error) => Err(format!("{message}; exit handler failed: {exit_error}")),
                }
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
    completion?;
    reporting?;
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

/// Parse the development CLI's entry form and route it through `execute_shared`.
pub fn run_shared_cli(arguments: impl IntoIterator<Item = String>) -> Result<ExitCode, String> {
    let mut args = arguments.into_iter();
    let first = args.next();
    match first.as_deref() {
        Some("--help") | Some("-h") => {
            println!("quench-node [-e CODE|SCRIPT]");
            return Ok(ExitCode::SUCCESS);
        }
        Some("--version") | Some("-v") => {
            println!("v22.0.0");
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }

    let input = match first.as_deref() {
        Some("-e") | Some("--eval") => SharedInput::Eval(args.next().unwrap_or_default()),
        Some(path) => SharedInput::File {
            path: PathBuf::from(path),
            exec_argv: Vec::new(),
            goal: EntryGoal::Node,
        },
        None => SharedInput::Eval(String::new()),
    };
    execute_shared(input, std::env::args().collect()).map(SharedCompletion::exit_code)
}
