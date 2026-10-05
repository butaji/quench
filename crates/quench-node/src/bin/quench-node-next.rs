use quench_node::NodeHost;
use quench_runtime::ops::RealmId;
use rqj::{Engine, ExecutionRequest, Runtime, SourceKind};
use std::process::ExitCode;

const UNSETTLED_TOP_LEVEL_AWAIT_EXIT: u8 = 13;

fn main() -> ExitCode {
    let result = std::thread::Builder::new()
        .name("quench-node-next".into())
        .stack_size(rqj::WORKER_STACK_SIZE)
        .spawn(run)
        .map_err(|error| format!("runtime worker thread: {error}"))
        .and_then(|worker| {
            worker
                .join()
                .unwrap_or_else(|_| Err("runtime worker panicked".into()))
        });
    match result {
        Ok(status) => status,
        Err(error) => {
            eprintln!("quench-node-next: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let mut args = std::env::args().skip(1);
    let first = args.next();
    match first.as_deref() {
        Some("--help") | Some("-h") => {
            println!("quench-node-next [-e CODE|SCRIPT]");
            return Ok(ExitCode::SUCCESS);
        }
        Some("--version") | Some("-v") => {
            println!("v22.0.0-next");
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }
    let (source, name, kind, entry) = match first.as_deref() {
        Some("-e") | Some("--eval") => (
            args.next().ok_or("missing source after -e")?,
            "<eval>".to_owned(),
            SourceKind::Script,
            None,
        ),
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            let kind = NodeHost::source_kind(&path)?;
            let is_module = kind == SourceKind::Module;
            let source = if is_module {
                std::fs::read_to_string(&path).map_err(|error| error.to_string())?
            } else {
                String::new()
            };
            (
                source,
                path.to_string_lossy().into_owned(),
                kind,
                if is_module { None } else { Some(path) },
            )
        }
        None => return Err("usage: quench-node-next [-e CODE|SCRIPT]".into()),
    };
    let host = NodeHost::new(RealmId::ROOT, std::env::args().collect());
    let host = match entry {
        Some(path) => host.with_commonjs_entry(path),
        None => host,
    };
    let mut runtime = Runtime::new(host);
    let program = Engine::compile(ExecutionRequest {
        source: &source,
        name: &name,
        kind,
    })
    .map_err(|error| format!("{error:?}"))?;
    runtime
        .execute(&program)
        .map_err(|error| runtime.format_error(&program, &error))?;
    match runtime
        .module_evaluation_pending(&program)
        .map_err(|error| error.to_string())?
    {
        true => Ok(ExitCode::from(UNSETTLED_TOP_LEVEL_AWAIT_EXIT)),
        false => Ok(ExitCode::SUCCESS),
    }
}
