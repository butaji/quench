use rqj::{ExecutionRequest, Runtime, SystemHost};

fn main() {
    if let Err(error) = run() {
        eprintln!("quench-next: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let source = match args.next().as_deref() {
        Some("-e") | Some("--eval") => args.next().ok_or("missing source after -e")?,
        Some(path) => std::fs::read_to_string(path).map_err(|error| error.to_string())?,
        None => return Err("usage: quench-next [-e CODE|SCRIPT]".into()),
    };
    let name = args.next().unwrap_or_else(|| "<eval>".into());
    let mut runtime = Runtime::new(SystemHost);
    runtime
        .compile_and_execute(ExecutionRequest::script(&source, &name))
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
}
