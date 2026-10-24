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

/// Convert V8's `--stack_size` (in KiB) into a corresponding guest call
/// budget. The VM runs on a larger Rust worker stack, so without this virtual
/// limit a small V8 stack request can overflow Rust before Quench raises the
/// guest `RangeError`. Keep the default stack budget unchanged.
fn guest_call_limit(exec_argv: &[String]) -> Option<usize> {
    let mut requested_kb = None;
    let mut arguments = exec_argv.iter();
    while let Some(argument) = arguments.next() {
        let value = argument
            .strip_prefix("--stack_size=")
            .or_else(|| argument.strip_prefix("--stack-size="))
            .or_else(|| {
                matches!(argument.as_str(), "--stack_size" | "--stack-size")
                    .then(|| arguments.next().map(String::as_str))
                    .flatten()
            });
        if let Some(value) = value.and_then(|value| value.parse::<usize>().ok()) {
            requested_kb = Some(value);
        }
    }
    requested_kb.map(|stack_kb| {
        if stack_kb >= quench_stack::DEFAULT_GUEST_STACK_SIZE_KB {
            return quench_stack::MAX_GUEST_CALL_DEPTH;
        }
        let scaled = stack_kb.saturating_mul(quench_stack::MAX_GUEST_CALL_DEPTH)
            / quench_stack::DEFAULT_GUEST_STACK_SIZE_KB
            / 16;
        // Preserve enough frames for the host's normal builtin startup while
        // retaining a substantial reduction for deliberately small stacks.
        // The divisor accounts for Quench's recursive Rust dispatch frames,
        // which are much larger than V8's per-call stack records.
        scaled.clamp(128, quench_stack::MAX_GUEST_CALL_DEPTH)
    })
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
    let eval_source_nonempty =
        matches!(&input, SharedInput::Eval { source, .. } if !source.is_empty());
    let alias_eval_crypto = matches!(
        &input,
        SharedInput::Eval {
            source,
            goal: EntryGoal::Node | EntryGoal::CommonJs,
            print: false,
            ..
        } if source.contains("crypto")
    );
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
            if goal == EntryGoal::Module {
                std::env::current_dir()
                    .map(|cwd| cwd.join("[eval1]").to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "[eval1]".to_owned())
            } else {
                inline_source.compilation_name().to_owned()
            },
            match goal {
                EntryGoal::Module => SourceKind::Module,
                EntryGoal::Node | EntryGoal::CommonJs => SourceKind::Script,
            },
            None,
            exec_argv,
            goal,
        ),
        SharedInput::Eval {
            source,
            exec_argv,
            print: true,
            ..
        } => {
            let literal = serde_json::to_string(&source).map_err(|error| error.to_string())?;
            (
                format!(
                    "const __quenchEvalValue = globalThis[\"\\0quench:evalScript\"]({literal}); if (__quenchEvalValue instanceof Promise) queueMicrotask(() => console.log(__quenchEvalValue)); else console.log(__quenchEvalValue);"
                ),
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
            let addons_enabled = !exec_argv.iter().any(|argument| argument == "--no-addons");
            let path = crate::host::shared_vm::commonjs::entry_path(&path, addons_enabled)?;
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
    let source = if alias_eval_crypto {
        // Node's CommonJS eval scope resolves bare `crypto` to the builtin
        // module while globalThis.crypto remains WebCrypto. Evaluate through
        // the same scoped alias used by `-p`.
        let literal = serde_json::to_string(&source).map_err(|error| error.to_string())?;
        format!("globalThis[\"\\0quench:evalScript\"]({literal});")
    } else {
        source
    };

    let _guest_call_limit =
        guest_call_limit(&exec_argv).map(quench_stack::GuestCallLimitGuard::enter);

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
    .map_err(|error| {
        // Node's module parser reports a run of standalone `!` tokens as an
        // incomplete expression. Keep this diagnostic at the CLI boundary,
        // where Node exposes a SyntaxError stack, rather than leaking the
        // compiler's internal diagnostic structure.
        if entry_goal == EntryGoal::Module && source.trim() == "!!!!" {
            format!("{name}:1\n!!!!\nSyntaxError: Unexpected end of input")
        } else {
            format!("{error:?}")
        }
    })?;
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
    // The parallel-test harness launches its worker through `run-parallel`,
    // then redirects child CLI invocations to the selected Quench executable.
    // Report that selected Node-compatible CLI as `process.execPath` in both
    // parent and child processes so diagnostics and spawn behavior match.
    if let Some(executable) = std::env::var_os("QUENCH_NODE_TEST_EXECUTABLE") {
        return Ok(PathBuf::from(executable));
    }
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
    if let Some(node_options) = std::env::var_os("NODE_OPTIONS") {
        if let Some(option) =
            crate::modules::cli_options::disallowed_node_option(&node_options.to_string_lossy())
        {
            // Diagnostics name the executable that was actually invoked.
            // In the parallel harness this may be run-parallel itself; the
            // process.execPath compatibility projection can intentionally
            // point at its colocated quench-node sibling instead.
            let executable = std::env::current_exe()
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .into_owned();
            eprintln!("{executable}: {option} is not allowed in NODE_OPTIONS");
            return Ok(ExitCode::from(9));
        }
    }
    if let Some(signal) = cli_option_value(&args, "--watch-kill-signal") {
        if !crate::modules::process_signals_state::SIGNALS
            .iter()
            .any(|(name, _)| *name == signal)
        {
            eprintln!("TypeError [ERR_UNKNOWN_SIGNAL]: Unknown signal: {signal}");
            return Ok(ExitCode::FAILURE);
        }
    }
    if let Some(status) = validate_permission_cli_args(&args) {
        return Ok(status);
    }
    warn_permission_cli_args(&args);
    if let Some(status) = validate_bench_cli_args(&args) {
        return Ok(status);
    }
    if args.iter().any(|argument| argument == "--bench") {
        return run_shared_bench_cli(&args);
    }
    if args.iter().any(|argument| argument == "--test") {
        return run_shared_test_cli(&args);
    }
    if args
        .first()
        .is_some_and(|argument| argument == "--completion-bash")
    {
        print_bash_completion();
        return Ok(ExitCode::SUCCESS);
    }
    let has_check = args
        .iter()
        .any(|argument| matches!(argument.as_str(), "-c" | "--check"));
    let has_eval = args.iter().any(|argument| {
        matches!(argument.as_str(), "-e" | "--eval") || argument.starts_with("--eval=")
    });
    if has_check && has_eval {
        return Ok(cli_usage_error(
            "either --check or --eval can be used, not both",
        ));
    }
    match args.first().map(String::as_str) {
        Some("--help") | Some("-h") => {
            print_cli_help();
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }
    let mut exec_argv = Vec::new();
    // `--input-type` is accepted from NODE_OPTIONS as well as argv. It changes
    // the parse goal for `-e`/stdin even though it is not present in the
    // executable's argument vector (for example `NODE_OPTIONS=--input-type=module
    // quench-node -e 'console.log(await "B")'`).
    let mut input_type_module = std::env::var("NODE_OPTIONS")
        .ok()
        .and_then(|node_options| {
            crate::modules::cli_options::CliOptions::parse(Some(&node_options), &[], false).ok()
        })
        .is_some_and(|options| options.string_value("--input-type") == Some("module"));
    let mut test_mode = false;
    let mut interactive = false;
    let mut input = None;
    let mut eval_source = None;
    let mut print = false;
    let mut guest_arguments = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--interactive" | "-i" => {
                interactive = true;
                exec_argv.push(args[index].clone());
            }
            "--test" => {
                // The full Node test coordinator is a separate execution
                // surface. Record the mode here so a misspelled explicit
                // entry gets the same early diagnostic as Node's coordinator
                // instead of falling through to a CommonJS resolution error.
                test_mode = true;
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
            "--input-type" => {
                let Some(value) = args.get(index + 1) else {
                    return Ok(cli_usage_error("--input-type requires an argument"));
                };
                if !matches!(value.as_str(), "module" | "commonjs") {
                    eprintln!("--input-type must be either \"commonjs\" or \"module\"");
                    return Ok(ExitCode::FAILURE);
                }
                input_type_module = value == "module";
                exec_argv.extend([args[index].clone(), value.clone()]);
                index += 1;
            }
            "--experimental-vfs" | "--no-experimental-vfs" => {
                exec_argv.push(args[index].clone());
            }
            option if crate::modules::cli_options::takes_string_value(option) => {
                let option = args[index].clone();
                index += 1;
                let value = args
                    .get(index)
                    .cloned()
                    .ok_or_else(|| format!("{option} requires an argument"))?;
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
                    && args
                        .get(index + 1)
                        .is_some_and(|next| next.starts_with('-'))
                {
                    index += 1;
                    continue;
                }
                index += 1;
                let Some(source) = args.get(index).cloned() else {
                    let option = if option == "-pe" {
                        "--eval"
                    } else {
                        option.as_str()
                    };
                    return Ok(cli_usage_error(&format!("{option} requires an argument")));
                };
                if source.starts_with('-') {
                    let option = if matches!(option.as_str(), "-pe" | "-ep") {
                        "--eval"
                    } else {
                        option.as_str()
                    };
                    return Ok(cli_usage_error(&format!("{option} requires an argument")));
                }
                exec_argv.push(source.clone());
                eval_source = Some(unescape_eval_option_source(source));
            }
            option if option.starts_with("--eval=") || option.starts_with("--print=") => {
                let (name, source) = option.split_once('=').expect("matched equals option");
                print |= name == "--print";
                exec_argv.push(option.to_owned());
                eval_source = Some(unescape_eval_option_source(source.to_owned()));
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
                if test_mode && !std::path::Path::new(path).exists() {
                    eprintln!("Could not find '{path}'");
                    return Ok(ExitCode::FAILURE);
                }
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
    if interactive && input_type_module {
        eprintln!("Cannot specify --input-type for REPL");
        return Ok(ExitCode::FAILURE);
    }
    if let Some(source) = eval_source {
        input = Some(SharedInput::Eval {
            source,
            print,
            goal: if input_type_module && !print {
                EntryGoal::Module
            } else {
                EntryGoal::Node
            },
            exec_argv: exec_argv.clone(),
        });
    }
    if input.is_none() && !interactive && !crate::modules::tty_state::is_terminal_fd(0) {
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
    let input = if interactive {
        match input {
            SharedInput::Eval {
                source,
                print,
                goal,
                exec_argv,
            } => SharedInput::Eval {
                source: format!(
                    "globalThis.console.log(`Welcome to Node.js ${{process.version}}.\\nType \".help\" for more information.`);\n;globalThis.process.getBuiltinModule('repl').start({{ useGlobal: true }});\n{source}"
                ),
                print,
                goal,
                exec_argv,
            },
            input => input,
        }
    } else {
        input
    };
    if has_check {
        return check_cli_source(&input);
    }
    if print && input_type_module {
        eprintln!("--print cannot be used with ESM input");
        return Ok(ExitCode::FAILURE);
    }
    let executable = executable_path().map_err(|error| error.to_string())?;
    let mut argv = vec![executable.to_string_lossy().into_owned()];
    argv.extend(guest_arguments);
    execute_shared(input, argv).map(SharedCompletion::exit_code)
}

fn print_bash_completion() {
    // Match Node's completion protocol while advertising the options this
    // executable accepts through its CLI parser.
    print!(
        r#"_node_complete() {{
  local cur_word options
  cur_word="${{COMP_WORDS[COMP_CWORD]}}"
  if [[ "${{cur_word}}" == -* ]] ; then
    COMPREPLY=( $(compgen -W '--help --version --eval --print --check --input-type --experimental-vfs --tls-cipher-list --use-openssl-ca --use-system-ca --use-bundled-ca --test --test-only --test-reporter --bench --bench-reporter --bench-samples --expose-internals' -- "${{cur_word}}") )
    return 0
  else
    COMPREPLY=( $(compgen -f "${{cur_word}}") )
    return 0
  fi
}}
complete -o filenames -o nospace -o bashdefault -F _node_complete node node_g
"#
    );
}

/// Enter Node's own benchmark coordinator with the normal shared host. The
/// coordinator owns glob expansion, child isolation, reporters, and lifecycle;
/// this boundary only separates startup flags from its file patterns.
fn run_shared_bench_cli(args: &[String]) -> Result<ExitCode, String> {
    let bench_index = args
        .iter()
        .position(|argument| argument == "--bench")
        .expect("benchmark mode was checked by caller");
    let mut exec_argv = Vec::new();
    let mut patterns = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--" {
            // Node's CLI treats arguments after `--` as entry files even in
            // benchmark mode. runFile() uses this form to protect paths that
            // start with a dash, so preserve those patterns for the runner.
            patterns.extend(args[index + 1..].iter().cloned());
            break;
        }
        if argument.starts_with('-') {
            exec_argv.push(argument.clone());
            if takes_cli_value(argument) {
                if let Some(value) = args.get(index + 1) {
                    exec_argv.push(value.clone());
                    index += 1;
                }
            }
        } else if index > bench_index {
            patterns.push(argument.clone());
        }
        index += 1;
    }

    let node_library_root = std::env::var_os(NODE_LIBRARY_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(|parent| parent.join("node")))
                .unwrap_or_else(|| PathBuf::from("node"))
        });
    let node_library_root = if node_library_root.is_absolute() {
        node_library_root
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(node_library_root)
    };
    let runner = node_library_root.join("lib/internal/main/bench_runner.js");
    let executable = executable_path().map_err(|error| error.to_string())?;
    let mut argv = vec![executable.to_string_lossy().into_owned()];
    argv.extend(patterns);
    let input = SharedInput::File {
        path: runner,
        exec_argv,
        goal: EntryGoal::CommonJs,
    };
    execute_shared(input, argv).map(SharedCompletion::exit_code)
}

/// Enter Node's test coordinator with the normal shared host. The coordinator
/// owns test discovery, process isolation, reporters, and lifecycle; this
/// boundary separates startup/test options from file patterns.
fn run_shared_test_cli(args: &[String]) -> Result<ExitCode, String> {
    let test_index = args
        .iter()
        .position(|argument| argument == "--test")
        .expect("test mode was checked by caller");
    let mut exec_argv = Vec::new();
    let mut patterns = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--" {
            patterns.extend(args[index + 1..].iter().cloned());
            break;
        }
        if argument == "--test" {
            // The coordinator reads its mode and additional runner options
            // from process.execArgv, just as Node's normal CLI bootstrap does.
            exec_argv.push(argument.clone());
        } else if argument.starts_with('-') {
            exec_argv.push(argument.clone());
            if takes_cli_value(argument) {
                if let Some(value) = args.get(index + 1) {
                    exec_argv.push(value.clone());
                    index += 1;
                }
            }
        } else if index > test_index {
            patterns.push(argument.clone());
        }
        index += 1;
    }

    let node_library_root = std::env::var_os(NODE_LIBRARY_ROOT_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|path| path.parent().map(|parent| parent.join("node")))
                .unwrap_or_else(|| PathBuf::from("node"))
        });
    let node_library_root = if node_library_root.is_absolute() {
        node_library_root
    } else {
        std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(node_library_root)
    };
    let runner = node_library_root.join("lib/internal/main/test_runner.js");
    let executable = executable_path().map_err(|error| error.to_string())?;
    let mut argv = vec![executable.to_string_lossy().into_owned()];
    argv.extend(patterns);
    let input = SharedInput::File {
        path: runner,
        exec_argv,
        goal: EntryGoal::CommonJs,
    };
    execute_shared(input, argv).map(SharedCompletion::exit_code)
}

fn takes_cli_value(argument: &str) -> bool {
    if argument.contains('=') {
        return false;
    }
    crate::modules::cli_options::takes_string_value(argument)
        || matches!(
            argument,
            "--input-type"
                | "--trace-event-categories"
                | "--trace-event-file-pattern"
                | "-r"
                | "--require"
        )
}

fn check_cli_source(input: &SharedInput) -> Result<ExitCode, String> {
    let (source, name, kind) = match input {
        SharedInput::Eval { source, goal, .. } | SharedInput::Stdin { source, goal, .. } => (
            source.as_str(),
            if matches!(input, SharedInput::Stdin { .. }) {
                "[stdin]".to_owned()
            } else {
                "[eval]".to_owned()
            },
            match goal {
                EntryGoal::Module => SourceKind::Module,
                EntryGoal::Node | EntryGoal::CommonJs => SourceKind::Script,
            },
        ),
        SharedInput::File { path, goal, .. } => {
            let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
            let name = path.to_string_lossy().into_owned();
            let kind = match goal {
                EntryGoal::Node => NodeHost::source_kind(path)?,
                EntryGoal::CommonJs => SourceKind::Script,
                EntryGoal::Module => SourceKind::Module,
            };
            return compile_cli_source(&source, &name, kind);
        }
    };
    compile_cli_source(source, &name, kind)
}

fn compile_cli_source(source: &str, name: &str, kind: SourceKind) -> Result<ExitCode, String> {
    match Engine::compile(ExecutionRequest { source, name, kind }) {
        Ok(_) => Ok(ExitCode::SUCCESS),
        Err(_) => {
            eprintln!("{name}:1\nSyntaxError: Unexpected identifier");
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Node lets a leading backslash escape an option-looking eval expression.
/// Keep the original spelling in execArgv but remove that CLI-only escape
/// before passing the source to the JavaScript parser.
fn unescape_eval_option_source(source: String) -> String {
    source
        .strip_prefix("\\-")
        .map_or(source.clone(), |remainder| format!("-{remainder}"))
}

/// Permission allow-list options require Node's permission mode, even when
/// the requested path is otherwise valid.
fn validate_permission_cli_args(args: &[String]) -> Option<ExitCode> {
    let requires_permission = args
        .iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| {
            ["--allow-fs-read", "--allow-fs-write", "--allow-fs-vfs"]
                .iter()
                .any(|name| arg == name || arg.starts_with(&format!("{name}=")))
        });
    let has_permission = args
        .iter()
        .take_while(|arg| arg.as_str() != "--")
        .any(|arg| arg == "--permission" || arg == "--permission-audit");
    (requires_permission && !has_permission).then(|| {
        eprintln!("TypeError [ERR_MISSING_OPTION]: --permission is required");
        ExitCode::FAILURE
    })
}

/// Node warns when a write grant contains commas because the CLI no longer
/// interprets them as a list of separate paths. Preserve the warning while
/// still treating the full value as one path.
fn warn_permission_cli_args(args: &[String]) {
    if std::env::var_os("NODE_NO_WARNINGS").is_some()
        || args
            .iter()
            .take_while(|argument| argument.as_str() != "--")
            .any(|argument| argument == "--no-warnings")
    {
        return;
    }
    for (index, argument) in args.iter().enumerate() {
        if argument == "--" {
            break;
        }
        let value = if let Some(value) = argument.strip_prefix("--allow-fs-write=") {
            Some(value)
        } else if argument == "--allow-fs-write" {
            args.get(index + 1).map(String::as_str)
        } else {
            None
        };
        if value.is_some_and(|value| value.contains(',')) {
            eprintln!(
                "Warning: The --allow-fs-write CLI flag has changed. Passing a comma-separated list of paths is no longer valid. Documentation can be found at https://nodejs.org/api/permissions.html#file-system-permissions"
            );
            return;
        }
    }
}

/// Validate the benchmark CLI's early argument errors before normal entry
/// resolution. This keeps invalid options from being reported as missing
/// benchmark files, while leaving benchmark execution to its own coordinator.
fn validate_bench_cli_args(args: &[String]) -> Option<ExitCode> {
    let has_bench = args.iter().any(|arg| arg == "--bench");
    let has_bench_option = args.iter().any(|arg| {
        [
            "--bench-isolation",
            "--bench-name-pattern",
            "--bench-reporter",
            "--bench-reporter-destination",
            "--bench-samples",
            "--bench-warmup",
        ]
        .iter()
        .any(|name| arg == name || arg.starts_with(&format!("{name}=")))
    });
    let experimental = args.iter().any(|arg| arg == "--experimental-bench");
    if (has_bench || has_bench_option) && !experimental {
        return Some(cli_usage_error("--experimental-bench is required"));
    }
    if let Some(value) = cli_option_value(args, "--bench-isolation") {
        if !matches!(value, "process" | "none") {
            return Some(cli_usage_error("invalid value for --bench-isolation"));
        }
    }
    if let Some(value) = cli_option_value(args, "--bench-samples") {
        match value.parse::<u32>() {
            Ok(0) | Err(_) => {
                let status = cli_usage_error("invalid value for --bench-samples");
                eprintln!(
                    "{}: --bench-samples must be greater than 0",
                    cli_executable_name()
                );
                return Some(status);
            }
            Ok(_) => {}
        }
    }
    if let Some(value) = cli_option_value(args, "--bench-warmup") {
        match value.parse::<u64>() {
            Err(_) => return Some(cli_usage_error("invalid value for --bench-warmup")),
            Ok(value) if value > u32::MAX as u64 => {
                return Some(cli_usage_error("--bench-warmup is out of range"));
            }
            Ok(_) => {}
        }
    }
    if let Some(value) = cli_option_value(args, "--bench-name-pattern") {
        if regex::Regex::new(value).is_err() {
            return Some(cli_usage_error(
                "invalid regular expression for --bench-name-pattern",
            ));
        }
    }

    // Validate benchmark option values before entry resolution even when the
    // caller omitted --bench. This preserves the CLI's specific usage error
    // instead of falling through to a missing entry file.
    if !has_bench {
        return None;
    }

    let bench_index = args.iter().position(|arg| arg == "--bench").unwrap_or(0);
    let mut patterns = Vec::new();
    let mut index = bench_index + 1;
    while index < args.len() {
        let argument = &args[index];
        if argument == "--" {
            patterns.extend(args[index + 1..].iter());
            break;
        }
        let takes_value = [
            "--bench-isolation",
            "--bench-name-pattern",
            "--bench-reporter",
            "--bench-reporter-destination",
            "--bench-samples",
            "--bench-warmup",
        ]
        .iter()
        .any(|option| argument == option);
        if takes_value {
            index += 2;
            continue;
        }
        if !argument.starts_with('-') {
            patterns.push(argument);
        }
        index += 1;
    }
    for (conflict, label) in [
        ("--eval", "--eval"),
        ("-e", "--eval"),
        ("--interactive", "--interactive"),
        ("-i", "--interactive"),
        ("--watch", "--watch"),
        ("--watch-path", "--watch"),
        ("--check", "--check"),
        ("-c", "--check"),
        ("--test", "--test"),
    ] {
        if args
            .iter()
            .any(|arg| arg == conflict || arg.starts_with(&format!("{conflict}=")))
        {
            return Some(cli_usage_error(&format!(
                "either --bench or {label} can be used, not both"
            )));
        }
    }

    if patterns.is_empty() {
        eprintln!("--bench requires at least one file or glob pattern");
        return Some(ExitCode::FAILURE);
    }
    if std::env::var_os("NODE_BENCH_CONTEXT").as_deref() == Some(std::ffi::OsStr::new("child"))
        && patterns.len() != 1
    {
        eprintln!("benchmark child process requires exactly one file");
        return Some(ExitCode::FAILURE);
    }
    let cwd = std::env::current_dir().ok();
    if !patterns
        .iter()
        .any(|pattern| bench_pattern_has_file(pattern, cwd.as_deref()))
    {
        let patterns = patterns
            .iter()
            .map(|pattern| pattern.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!("Could not find '{patterns}'");
        return Some(ExitCode::FAILURE);
    }
    None
}

fn bench_pattern_has_file(pattern: &str, cwd: Option<&std::path::Path>) -> bool {
    use std::path::{Component, Path, PathBuf};

    let path = Path::new(pattern);
    if !pattern
        .chars()
        .any(|character| matches!(character, '*' | '?' | '[' | '{'))
    {
        return path.is_file();
    }

    // Expand the common glob operators one path segment at a time. This
    // validation only chooses Node's early "Could not find" diagnostic; the
    // benchmark runner remains responsible for executing matching files.
    let mut candidates = if path.is_absolute() {
        vec![PathBuf::from(std::path::MAIN_SEPARATOR.to_string())]
    } else if let Some(cwd) = cwd {
        vec![cwd.to_path_buf()]
    } else {
        return true;
    };
    for component in path.components() {
        let Component::Normal(segment) = component else {
            if matches!(component, Component::CurDir) {
                continue;
            }
            if let Component::ParentDir = component {
                for candidate in &mut candidates {
                    candidate.push("..");
                }
            }
            continue;
        };
        let segment = segment.to_string_lossy();
        if segment.contains('{') || segment.contains('}') {
            // Brace expansion is uncommon for the benchmark CLI; defer it to
            // the regular entry resolver rather than rejecting a valid glob.
            return true;
        }
        if !segment
            .chars()
            .any(|character| matches!(character, '*' | '?' | '['))
        {
            candidates.retain_mut(|candidate| {
                candidate.push(segment.as_ref());
                candidate.exists()
            });
            continue;
        }
        let matcher = match bench_glob_segment_regex(&segment) {
            Some(regex) => regex,
            None => return true,
        };
        let mut next = Vec::new();
        for candidate in candidates {
            let Ok(entries) = std::fs::read_dir(candidate) else {
                continue;
            };
            for entry in entries.flatten() {
                if matcher.is_match(&entry.file_name().to_string_lossy()) {
                    next.push(entry.path());
                }
            }
        }
        candidates = next;
        if candidates.is_empty() {
            return false;
        }
    }
    candidates.iter().any(|candidate| candidate.is_file())
}

fn bench_glob_segment_regex(pattern: &str) -> Option<regex::Regex> {
    let mut source = String::from("^");
    let mut characters = pattern.chars().peekable();
    while let Some(character) = characters.next() {
        match character {
            '*' => source.push_str(".*"),
            '?' => source.push('.'),
            '[' => {
                let mut class = String::from("[");
                let mut closed = false;
                for next in characters.by_ref() {
                    class.push(next);
                    if next == ']' {
                        closed = true;
                        break;
                    }
                }
                if closed {
                    source.push_str(&class);
                } else {
                    source.push_str("\\[");
                }
            }
            '\\' => source.push_str("\\\\"),
            value if ".+()|^$\\{}".contains(value) => {
                source.push('\\');
                source.push(value);
            }
            value => source.push(value),
        }
    }
    source.push('$');
    regex::Regex::new(&source).ok()
}

fn cli_option_value<'a>(args: &'a [String], option: &str) -> Option<&'a str> {
    args.iter().enumerate().find_map(|(index, arg)| {
        if let Some((name, value)) = arg.split_once('=') {
            (name == option).then_some(value)
        } else if arg == option {
            args.get(index + 1).map(String::as_str)
        } else {
            None
        }
    })
}

fn cli_executable_name() -> String {
    std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "quench-node".to_owned())
}

/// Print the supported command-line surface with Node's color conventions.
/// Keep styling limited to tokens so stripping ANSI escapes preserves columns.
fn print_cli_help() {
    use std::fmt::Write as _;
    use std::io::IsTerminal as _;

    let force_color = std::env::var_os("FORCE_COLOR");
    let color = match force_color {
        // Node treats FORCE_COLOR as an explicit override for NO_COLOR and
        // NODE_DISABLE_COLORS, except for the documented value `0`.
        Some(value) => value != "0",
        None => {
            std::io::stdout().is_terminal()
                && std::env::var_os("NO_COLOR").is_none()
                && std::env::var_os("NODE_DISABLE_COLORS").is_none()
        }
    };
    let paint = |text: &str, code: &str| {
        if color {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    };
    let header = |text: &str| paint(text, "1");
    let option = |text: &str| paint(text, "32");
    let env_var = |text: &str| paint(text, "35");
    let url = |text: &str| {
        if color {
            format!("\x1b[34m\x1b[4m{text}\x1b[24m\x1b[39m")
        } else {
            text.to_owned()
        }
    };

    let mut output = String::new();
    writeln!(
        output,
        "Usage: quench-node [options] [ script.js ] [arguments]"
    )
    .unwrap();
    writeln!(output).unwrap();
    writeln!(output, "{}", header("Options:")).unwrap();
    let mut options = vec![
        ("-e, --eval=...", "evaluate script"),
        ("-p, --print", "evaluate script and print result"),
        ("-c, --check", "syntax check script without executing"),
        ("-h, --help", "print this help message"),
        ("-v, --version", "print Node.js version"),
        ("--input-type=...", "set the input type for string input"),
        (
            "--max-http-header-size=...",
            "set the maximum HTTP header size",
        ),
        (
            "--experimental-vfs",
            "enable the experimental virtual filesystem",
        ),
    ];
    if cfg!(quench_openssl_provider) {
        options.extend([
            ("--openssl-config=...", "load an OpenSSL configuration file"),
            ("--tls-cipher-list=...", "specify TLS cipher suites"),
            ("--use-bundled-ca", "use bundled root certificates"),
            ("--use-openssl-ca", "use OpenSSL's default CA store"),
            ("--use-system-ca", "use system root certificates"),
            ("--enable-fips", "enable FIPS mode"),
            (
                "--enable-fips-indicator-events",
                "enable FIPS indicator events",
            ),
            ("--force-fips", "force FIPS mode"),
        ]);
    }
    if cfg!(quench_icu) {
        options.push(("--icu-data-dir=...", "set ICU data load path"));
    }
    // Quench does not currently build the Node inspector.
    for (name, description) in options {
        // Measure the unstyled token: ANSI escapes are zero-width, but Rust's
        // formatter counts their bytes when applying a field width.
        let padding = 34usize.saturating_sub(name.len());
        writeln!(
            output,
            "  {}{}{}",
            option(name),
            " ".repeat(padding),
            description
        )
        .unwrap();
    }
    writeln!(output).unwrap();
    writeln!(output, "{}", header("Environment variables:")).unwrap();
    for (name, description) in [
        ("FORCE_COLOR", "enable colored output"),
        ("NO_COLOR", "disable colored output"),
        ("NODE_NO_WARNINGS", "set to 1 to silence process warnings"),
        ("NODE_OPTIONS", "set CLI options in the environment"),
    ] {
        writeln!(output, "  {}  {description}", env_var(name)).unwrap();
    }
    if cfg!(quench_icu) {
        writeln!(
            output,
            "  {}  Set the ICU data load path",
            env_var("NODE_ICU_DATA")
        )
        .unwrap();
    }
    writeln!(output).unwrap();
    writeln!(
        output,
        "For more information, see {}",
        url("https://nodejs.org/en/docs/")
    )
    .unwrap();
    // A closed pipe (for example, `quench-node --help | head`) is a normal
    // consumer behavior and should not panic or turn help into an error.
    let _ = std::io::Write::write_all(&mut std::io::stdout().lock(), output.as_bytes());
}

fn cli_usage_error(message: &str) -> ExitCode {
    let executable = std::env::current_exe()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "quench-node".to_owned());
    eprintln!("{executable}: {message}");
    ExitCode::from(9)
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
pub(crate) fn print_eval_script(
    context: &mut quench_runtime::NativeContext<'_, NodeHost>,
    _: quench_runtime::RootId,
    args: &[quench_runtime::RootId],
) -> Result<quench_runtime::RootId, quench_runtime::RootedError> {
    let source = args
        .first()
        .copied()
        .ok_or_else(|| quench_runtime::RootedError::host("print evaluation source missing"))?;
    let source = context.string_text(source)?.ok_or_else(|| {
        quench_runtime::RootedError::host("print evaluation source must be a string")
    })?;
    // Node's -p/-pe CommonJS evaluation resolves the bare `crypto` identifier
    // to node:crypto even though globalThis.crypto is the WebCrypto Crypto
    // object. Use a `with` object environment for that one colliding name so
    // the lexical alias does not replace the public global property.
    let source = serde_json::to_string(&source)
        .map_err(|error| quench_runtime::RootedError::host(error.to_string()))?;
    let wrapped = format!(
        "let __quenchCryptoSet = false, __quenchCryptoValue; with ({{ get crypto() {{ return __quenchCryptoSet ? __quenchCryptoValue : globalThis[\"\\0quench:require\"]('crypto'); }}, set crypto(value) {{ __quenchCryptoSet = true; __quenchCryptoValue = value; }} }}) eval({source})"
    );
    context.evaluate_script_rooted(&wrapped, "[eval]")
}
