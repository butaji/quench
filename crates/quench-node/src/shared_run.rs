//! Shared-VM entry execution used by the development CLI and inventory worker.

use crate::host::NodeHost;
use quench_runtime::{Engine, ExecutionRequest, Runtime, SourceKind};
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
            execute_shared_on_worker(input, argv)
        })
        .map_err(|error| format!("runtime worker thread: {error}"))?
        .join()
        .unwrap_or_else(|_| Err("runtime worker panicked".into()))
}

fn execute_shared_on_worker(
    input: SharedInput,
    argv: Vec<String>,
) -> Result<SharedCompletion, String> {
    let eval_builtin_globals = matches!(&input, SharedInput::Eval { .. });
    let eval_source_nonempty = matches!(&input, SharedInput::Eval { source, .. } if !source.is_empty());
    let inline_source = match &input {
        SharedInput::Stdin { .. } => crate::host::node_host::InlineSource::Stdin,
        _ => crate::host::node_host::InlineSource::Eval,
    };
    let (source, name, kind, commonjs_entry, exec_argv, entry_goal) = match input {
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

    let mut host = NodeHost::new(argv).with_exec_argv(exec_argv)?;
    host.inline_source = inline_source;
    host.eval_builtin_globals = eval_builtin_globals;
    host.eval_source_nonempty = eval_source_nonempty;
    let host = match commonjs_entry {
        Some(path) => host.with_commonjs_entry_goal(path, entry_goal),
        None => host,
    };
    let shared_state = host.shared_state();
    {
        let mut state = shared_state.borrow_mut();
        let argv = state.process_argv.clone();
        let exec_argv = state.exec_argv.clone();
        state.trace.open_environment(argv.as_slice(), &exec_argv);
    }
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
    crate::host::resident_pages::map_touched_pages_only();
    let args = arguments.into_iter().collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("--help") | Some("-h") => {
            println!("quench-node [-e CODE|SCRIPT]");
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }
    let mut exec_argv = Vec::new();
    let mut input_type_module = false;
    let mut input = None;
    let mut eval_source = None;
    let mut print = false;
    let mut guest_arguments = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
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
            "-e" | "--eval" | "-pe" | "-ep" | "-p" | "--print" => {
                let option = args[index].clone();
                print |= matches!(option.as_str(), "-pe" | "-ep" | "-p" | "--print");
                exec_argv.push(option.clone());
                // Node's optional print argument treats an empty argument as
                // the first guest argv entry, unlike --eval's required argument.
                if matches!(option.as_str(), "-p" | "--print")
                    && args.get(index + 1).is_some_and(String::is_empty)
                {
                    eval_source = Some(String::new());
                    guest_arguments.extend_from_slice(&args[index + 1..]);
                    break;
                }
                // --print is also a switch when followed by --eval.
                if matches!(option.as_str(), "-p" | "--print")
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
            option if option.starts_with("--eval=") || option.starts_with("--print=") => {
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
    execute_shared(input, argv).map(SharedCompletion::exit_code)
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
