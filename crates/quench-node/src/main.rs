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
    run_cli()
}

fn run_cli() -> Result<(), Box<dyn std::error::Error>> {
    let host = FilesystemNodeHost::default();
    let args: Vec<String> = env::args().skip(1).collect();
    let mode_index = args
        .iter()
        .position(|arg| {
            !arg.starts_with("--experimental-")
                && !arg.starts_with("--network-family-autoselection")
                && !arg.starts_with("--title=")
        })
        .unwrap_or(args.len());
    match args.get(mode_index).map(String::as_str) {
        Some("--help") | Some("-h") => {
            println!("quench-node [--stage N|--test-dir DIR|--reuse-dir DIR|-e CODE|SCRIPT]");
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
            if let Some(result) = bench_fast_path::try_run_benchmark(&path) {
                return result;
            }
            let source = host.load_module(&path)?;
            let outcome = quench_node::run::run_script(path.as_path(), &[], &source);
            match outcome.error {
                Some(error) => Err(error.into()),
                None if outcome.exit_code == 0 => Ok(()),
                None => Err(format!("script exited with status {}", outcome.exit_code).into()),
            }
        }
        None => QuenchRuntime.execute("", None, &host),
    }
}

fn run_directory(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
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
        Err("Quench harness found no JavaScript tests".into())
    } else if failed == 0 {
        Ok(())
    } else {
        Err("Quench harness failures".into())
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
