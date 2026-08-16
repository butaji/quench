use std::{env, fs, path::PathBuf, sync::atomic::AtomicUsize};
use walkdir::WalkDir;
mod esm;
#[macro_use]
mod host_context;
mod js_runtime;
mod quickjs_backend;

mod polyfills;
use js_runtime::{FilesystemNodeHost, JsRuntime, NodeHost, QuenchRuntime, QuickJsRuntime};
#[rustfmt::skip]
static BOOTSTRAP_SOURCE: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| polyfills::node_compat().bootstrap_source());
static MKDTEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let host = FilesystemNodeHost::default();
    if !env::args().skip(1).any(|arg| arg == "--quickjs") {
        return run_quench_cli();
    }
    let mut args = cli_args().into_iter();
    let mode = args.next();
    if mode.as_deref() == Some("--help") || mode.as_deref() == Some("-h") {
        print_help();
        return Ok(());
    }
    if mode.as_deref() == Some("--stage") {
        let stage = args.next().unwrap_or_else(|| "0".into());
        return run_directory(&PathBuf::from(format!("tests/node-compat/stage-{stage}")));
    }
    if mode.as_deref() == Some("--test-dir") {
        let dir = PathBuf::from(
            args.next()
                .unwrap_or_else(|| "tests/node/test/parallel".into()),
        );
        return run_directory(&dir);
    }
    if mode.as_deref() == Some("--reuse-dir") {
        let dir = PathBuf::from(args.next().unwrap_or_else(|| "tests/node-compat".into()));
        return run_directory_reuse(&dir);
    }
    match mode.as_deref() {
        Some("-e") | Some("--eval") => {
            let runtime = QuickJsRuntime::new()?;
            runtime.execute(&args.next().unwrap_or_default(), None, &host)
        }
        Some(path) => {
            let path = host.resolve_module(path, None)?;
            let source = host.load_module(&path)?;
            let runtime = QuickJsRuntime::new()?;
            runtime.execute(&source, Some(path.as_path()), &host)
        }
        None => {
            let runtime = QuickJsRuntime::new()?;
            runtime.execute("", None, &host)
        }
    }
}
fn run_quench_cli() -> Result<(), Box<dyn std::error::Error>> {
    let host = FilesystemNodeHost::default();
    let args: Vec<String> = env::args()
        .skip(1)
        .filter(|arg| arg != "--quickjs")
        .collect();
    match args.first().map(String::as_str) {
        Some("--help") | Some("-h") => {
            println!("quench-node [--quickjs] [-e CODE|SCRIPT]");
            Ok(())
        }
        Some("-e") | Some("--eval") => {
            QuenchRuntime.execute(args.get(1).map_or("", String::as_str), None, &host)
        }
        Some("--stage") => {
            let stage = args.get(1).map(String::as_str).unwrap_or("0");
            run_quench_directory(&PathBuf::from(format!("tests/node-compat/stage-{stage}")))
        }
        Some("--test-dir") => {
            let dir = PathBuf::from(
                args.get(1)
                    .cloned()
                    .unwrap_or_else(|| "tests/node-compat".into()),
            );
            run_quench_directory(&dir)
        }
        Some(path) => {
            let path = host.resolve_module(path, None)?;
            let source = host.load_module(&path)?;
            QuenchRuntime.execute(&source, Some(path.as_path()), &host)
        }
        None => QuenchRuntime.execute("", None, &host),
    }
}

fn run_quench_directory(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if dir.is_file() {
        let source = fs::read_to_string(dir)?;
        return QuenchRuntime.execute(&source, Some(dir), &FilesystemNodeHost::default());
    }
    let mut failed = 0;
    let mut total = 0;
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "js" || extension == "mjs")
        {
            total += 1;
            let source = fs::read_to_string(entry.path())?;
            match QuenchRuntime.execute(&source, Some(entry.path()), &FilesystemNodeHost::default())
            {
                Ok(()) => println!("ok {}", entry.path().display()),
                Err(error) => {
                    failed += 1;
                    eprintln!("not ok {}: {error:?}", entry.path().display());
                }
            }
        }
    }
    println!("{total} tests, {} passed, {failed} failed", total - failed);
    if total == 0 {
        Err("Quench harness found no JavaScript tests".into())
    } else if failed == 0 {
        Ok(())
    } else {
        Err("Quench harness failures".into())
    }
}
fn cli_args() -> Vec<String> {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let fixture_flags = ["--help", "--stage", "--test-dir", "--reuse-dir", "--eval"];
    // Fixture flags are Node CLI flags; experimental switches are selected by
    // the JS polyfills and must not be mistaken for the fixture path.
    args.retain(|arg| !arg.starts_with("--") || fixture_flags.contains(&arg.as_str()));
    args
}
fn print_help() {
    println!("quench-node [--stage N|--test-dir DIR|--reuse-dir DIR|-e CODE|SCRIPT]\n  --reuse-dir reuses one rquickjs runtime with isolated contexts per script");
}
fn run_single_file(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(dir)?;
    let runtime = QuickJsRuntime::new()?;
    match runtime.execute(&source, Some(dir), &FilesystemNodeHost::default()) {
        Ok(()) => {
            println!("ok {}", dir.display());
            Ok(())
        }
        Err(error) => {
            eprintln!("not ok {}: {error:?}", dir.display());
            Err("Node test harness failures".into())
        }
    }
}
fn run_directory(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    if dir.is_file() {
        return run_single_file(dir);
    }
    run_directory_with_runtime(
        dir,
        false,
        "Node test harness found no JavaScript tests",
        "Node test harness failures",
    )
}
fn run_directory_reuse(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    run_directory_with_runtime(
        dir,
        true,
        "Node reusable harness found no JavaScript tests",
        "Node reusable harness failures",
    )
}
fn run_directory_with_runtime(
    dir: &PathBuf,
    reuse: bool,
    empty_error: &str,
    failure_error: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let shared_runtime = reuse.then(QuickJsRuntime::new).transpose()?;
    let mut failed = 0;
    let mut total = 0;
    for entry in WalkDir::new(dir).into_iter().filter_map(Result::ok) {
        if entry.file_type().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|e| e == "js" || e == "mjs")
        {
            total += 1;
            let source = fs::read_to_string(entry.path())?;
            let result = match shared_runtime.as_ref() {
                Some(runtime) => {
                    runtime.execute(&source, Some(entry.path()), &FilesystemNodeHost::default())
                }
                None => QuickJsRuntime::new()?.execute(
                    &source,
                    Some(entry.path()),
                    &FilesystemNodeHost::default(),
                ),
            };
            match result {
                Ok(()) => println!("ok {}", entry.path().display()),
                Err(error) => {
                    failed += 1;
                    eprintln!("not ok {}: {error:?}", entry.path().display());
                }
            }
        }
    }
    println!("{total} tests, {} passed, {failed} failed", total - failed);
    if total == 0 {
        Err(empty_error.into())
    } else if failed == 0 {
        Ok(())
    } else {
        Err(failure_error.into())
    }
}
#[cfg(test)]
mod tests {
    use super::{
        js_runtime::{FilesystemNodeHost, JsRuntime},
        QuickJsRuntime,
    };
    #[test]
    fn evaluates_javascript_source() {
        QuickJsRuntime::new()
            .unwrap()
            .execute(
                "if (1 + 1 !== 2) throw new Error('bad arithmetic');",
                None,
                &FilesystemNodeHost::default(),
            )
            .unwrap();
    }
    #[test]
    fn loads_node_compatibility_globals() {
        QuickJsRuntime::new()
            .unwrap()
            .execute(
                "if (typeof Buffer !== 'function') throw new Error('Buffer missing');",
                None,
                &FilesystemNodeHost::default(),
            )
            .unwrap();
    }
}
