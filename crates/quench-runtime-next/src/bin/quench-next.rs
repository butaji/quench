use rqj::{Engine, Runtime, SystemHost};

fn main() {
    if let Err(error) = run() {
        eprintln!("quench-next: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let first = args
        .next()
        .ok_or("usage: quench-next [--generic] [-e CODE|SCRIPT]")?;
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
        Engine::specialize_unspecialized(&source, &name)
    } else {
        Engine::specialize(&source, &name)
    }
    .map_err(|errors| format!("{errors:?}"))?;
    runtime
        .execute(&program)
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
}
