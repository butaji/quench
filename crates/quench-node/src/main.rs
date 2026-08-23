use quench_node::run::eval_script;
use quench_runtime::vm::OutputSink;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::AtomicUsize,
};
use walkdir::WalkDir;
mod bench_fast_path;
mod js_runtime;
mod polyfills;
use js_runtime::{FilesystemNodeHost, JsRuntime, NodeHost, QuenchRuntime};
static MKDTEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    run_quench_cli()
}
fn run_quench_cli() -> Result<(), Box<dyn std::error::Error>> {
    let host = FilesystemNodeHost::default();
    let args: Vec<String> = env::args().skip(1).collect();
    // Node CLI switches (--experimental-*, --network-family-autoselection,
    // --title=*) select runtime behavior in the polyfills and may precede the
    // mode. They must not be mistaken for the script path.
    let mode_index = args
        .iter()
        .position(|arg| {
            !arg.starts_with("--experimental-")
                && !arg.starts_with("--network-family-autoselection")
                && !arg.starts_with("--title=")
        })
        .unwrap_or(args.len());
    let mode = args.get(mode_index).map(String::as_str);
    match mode {
        Some("--help") | Some("-h") => {
            println!("quench-node [-e CODE|SCRIPT]");
            Ok(())
        }
        Some("-e") | Some("--eval") => {
            let source = args.get(mode_index + 1).map_or("", String::as_str);
            let sink: OutputSink = std::sync::Arc::new(|line| println!("{line}"));
            match eval_script(source, sink).error {
                Some(error) => Err(error.into()),
                None => Ok(()),
            }
        }
        Some("--stage") => run_directory(&PathBuf::from(format!(
            "tests/node-compat/stage-{}",
            args.get(mode_index + 1).map(String::as_str).unwrap_or("0")
        ))),
        Some("--test-dir") | Some("--reuse-dir") => run_directory(&PathBuf::from(
            args.get(mode_index + 1)
                .cloned()
                .unwrap_or_else(|| "tests/node-compat".into()),
        )),
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
    println!("quench-node [--stage N|--test-dir DIR|--reuse-dir DIR|-e CODE|SCRIPT]");
}
fn run_single_file(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(dir)?;
    match QuenchRuntime.execute(&source, Some(dir), &FilesystemNodeHost::default()) {
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
    _reuse: bool,
    empty_error: &str,
    failure_error: &str,
) -> Result<(), Box<dyn std::error::Error>> {
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
        QuenchRuntime,
    };
    #[test]
    fn evaluates_javascript_source() {
        QuenchRuntime
            .execute(
                "if (1 + 1 !== 2) throw new Error('bad arithmetic');",
                None,
                &FilesystemNodeHost::default(),
            )
            .unwrap();
    }
    #[test]
    fn loads_node_compatibility_globals() {
        QuenchRuntime
            .execute(
                "if (typeof Buffer !== 'function') throw new Error('Buffer missing');",
                None,
                &FilesystemNodeHost::default(),
            )
            .unwrap();
    }
}
