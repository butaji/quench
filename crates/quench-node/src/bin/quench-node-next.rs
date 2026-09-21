use rqj::{ExecutionRequest, Runtime, SystemHost};

fn main() {
    if let Err(error) = run() {
        eprintln!("quench-node-next: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let first = args.next();
    match first.as_deref() {
        Some("--help") | Some("-h") => {
            println!("quench-node-next [-e CODE|SCRIPT]");
            return Ok(());
        }
        Some("--version") | Some("-v") => {
            println!("v22.0.0-next");
            return Ok(());
        }
        _ => {}
    }
    let (source, name) = match first.as_deref() {
        Some("-e") | Some("--eval") => (
            args.next().ok_or("missing source after -e")?,
            "<eval>".to_owned(),
        ),
        Some(path) => (
            std::fs::read_to_string(path).map_err(|error| error.to_string())?,
            path.to_owned(),
        ),
        None => return Err("usage: quench-node-next [-e CODE|SCRIPT]".into()),
    };
    let mut runtime = Runtime::new(SystemHost);
    runtime
        .compile_and_execute(ExecutionRequest::script(&source, &name))
        .map(|_| ())
        .map_err(|error| format!("{error:?}"))
}
