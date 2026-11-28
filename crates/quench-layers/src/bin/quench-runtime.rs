use quench_runtime::{Engine, Runtime, SystemHost};

const USAGE: &str = "usage: quench-runtime [--generic] [-e CODE|SCRIPT]\n       quench-runtime compile ENTRY -o BINARY (quench compile --help for options)";

fn main() {
    if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "--help" || argument == "-h")
    {
        println!("{USAGE}");
        return;
    }
    if std::env::args_os()
        .nth(1)
        .is_some_and(|argument| argument == "compile")
    {
        let result: Result<std::process::ExitStatus, String> = std::env::current_exe().map_err(|error| error.to_string()).and_then(|executable| {
            let compiler = executable.with_file_name(if cfg!(windows) { "quench.exe" } else { "quench" });
            let mut command = std::process::Command::new(&compiler);
            command.args(std::env::args_os().skip(1));
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                let error = command.exec();
                Err(format!("Node compiler companion {}: {error}; build quench with --features aot", compiler.display()))
            }
            #[cfg(not(unix))]
            command.status().map_err(|error| format!("Node compiler companion {}: {error}; build quench with --features aot", compiler.display()))
        });
        match result {
            Ok(status) => std::process::exit(status.code().unwrap_or(1)),
            Err(error) => {
                eprintln!("quench-runtime: {error}");
                std::process::exit(1);
            }
        }
    }
    let result = std::thread::Builder::new()
        .name("quench".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(run)
        .map_err(|error| format!("runtime worker thread: {error}"))
        .and_then(|worker| {
            worker
                .join()
                .unwrap_or_else(|_| Err("runtime worker panicked".into()))
        });
    if let Err(error) = result {
        eprintln!("quench-runtime: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mut first = args.next().ok_or(USAGE)?;
    // A leading JIT mode flag (`--no-jit`) selects the process's tiers, as quench-node's does.
    if quench_runtime::JitMode::is_flag(&first) {
        let mut chosen = None;
        quench_runtime::JitMode::select(&mut chosen, &first)?;
        if let Some(mode) = chosen { mode.set_process_mode(); }
        first = args.next().ok_or(USAGE)?;
    }
    let generic = first == "--generic";
    let source_arg = if generic { args.next() } else { Some(first) };
    let source = match source_arg.as_deref() {
        Some("-e") | Some("--eval") => args.next().ok_or("missing source after -e")?,
        Some(path) => std::fs::read_to_string(path).map_err(|error| error.to_string())?,
        None => return Err("missing script after --generic".into()),
    };
    let name = args.next().unwrap_or_else(|| "<eval>".into());
    let mut runtime = Runtime::new(SystemHost);
    let program = if generic {
        Engine::specialize(&source, &name)
    } else {
        Engine::specialize(&source, &name)
    }
    .map_err(|errors| format!("{errors:?}"))?;
    runtime
        .execute(&program)
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
}

// Links the runtime layer crates (quench-layers).
extern crate quench_layers;
