use std::{env, fs, path::PathBuf};

use rquickjs::{Context, Runtime};
use walkdir::WalkDir;

const BOOTSTRAP: &str = include_str!("../polyfills/bootstrap.js");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mode = args.next();
    if mode.as_deref() == Some("--test-dir") {
        let dir = PathBuf::from(args.next().unwrap_or_else(|| "tests/node/test/parallel".into()));
        return run_directory(&dir);
    }
    let source = match mode.as_deref() {
        Some("-e") | Some("--eval") => args.next().unwrap_or_default(),
        Some(path) => fs::read_to_string(path)?,
        None => String::new(),
    };
    run_source(&source)
}

fn run_source(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::new()?;
    let context = Context::full(&runtime)?;
    context.with(|ctx| -> rquickjs::Result<()> {
        ctx.eval::<(), _>(BOOTSTRAP.as_bytes())?;
        ctx.eval::<(), _>(source.as_bytes())
    })?;
    Ok(())
}

fn run_directory(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let mut failed = 0;
    let mut total = 0;
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "js") {
            total += 1;
            let source = fs::read_to_string(entry.path())?;
            match run_source(&source) {
                Ok(()) => println!("ok {}", entry.path().display()),
                Err(error) => {
                    failed += 1;
                    eprintln!("not ok {}: {error}", entry.path().display());
                }
            }
        }
    }
    println!("{total} tests, {} passed, {failed} failed", total - failed);
    if failed == 0 { Ok(()) } else { Err("Node test harness failures".into()) }
}
