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
    },
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
        SharedInput::Eval {
            source,
            goal,
            exec_argv,
        } => (
            source,
            "<eval>".to_owned(),
            match goal {
                EntryGoal::Module => SourceKind::Module,
                EntryGoal::Node | EntryGoal::CommonJs => SourceKind::Script,
            },
            None,
            exec_argv,
            goal,
        ),
        SharedInput::File {
            path,
            exec_argv,
            goal,
        } => {
            let kind = match goal {
                EntryGoal::Node => NodeHost::source_kind(&path)?,
                EntryGoal::CommonJs => SourceKind::Script,
                EntryGoal::Module => SourceKind::Module,
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
                crate::modules::process_shared_vm::finish_requested_exit(&mut runtime, &program)
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

/// Parse the development CLI's entry form and route it through `execute_shared`.
pub fn run_shared_cli(arguments: impl IntoIterator<Item = String>) -> Result<ExitCode, String> {
    let args = arguments.into_iter().collect::<Vec<_>>();
    match args.first().map(String::as_str) {
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
    let mut exec_argv = Vec::new();
    let mut input_type_module = false;
    let mut input = None;
    let mut guest_arguments = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
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
            "-e" | "--eval" | "-pe" | "-ep" | "-p" | "--print" => {
                let option = args[index].clone();
                let print = matches!(option.as_str(), "-pe" | "-ep" | "-p" | "--print");
                index += 1;
                let source = args.get(index).cloned().unwrap_or_default();
                exec_argv.extend([option, source.clone()]);
                guest_arguments.extend_from_slice(args.get(index + 1..).unwrap_or_default());
                input = Some(SharedInput::Eval {
                    source: if print {
                        format!("console.log({source});")
                    } else {
                        source
                    },
                    goal: if input_type_module && !print {
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
                let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
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
    let input = input.unwrap_or_else(|| SharedInput::Eval {
        source: String::new(),
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
    execute_shared(input, argv).map(SharedCompletion::exit_code)
}
