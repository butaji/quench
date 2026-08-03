use std::{
    env, fs,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use rand::RngCore;
use rquickjs::{function::Func, Context, Runtime};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

mod host_context;

const BOOTSTRAP_PARTS: &[&str] = &[
    include_str!("../polyfills/bootstrap-parts/part-00.js"),
    include_str!("../polyfills/bootstrap-parts/part-01.js"),
    include_str!("../polyfills/bootstrap-parts/part-02.js"),
    include_str!("../polyfills/bootstrap-parts/part-03.js"),
    include_str!("../polyfills/bootstrap-parts/part-04.js"),
    include_str!("../polyfills/bootstrap-parts/part-05.js"),
    include_str!("../polyfills/bootstrap-parts/part-06.js"),
    include_str!("../polyfills/bootstrap-parts/part-07.js"),
    include_str!("../polyfills/bootstrap-parts/part-08.js"),
    include_str!("../polyfills/bootstrap-parts/part-09.js"),
    include_str!("../polyfills/bootstrap-parts/part-10.js"),
    include_str!("../polyfills/bootstrap-parts/part-11.js"),
    include_str!("../polyfills/bootstrap-parts/part-12.js"),
    include_str!("../polyfills/bootstrap-parts/part-13.js"),
    include_str!("../polyfills/bootstrap-parts/part-14.js"),
    include_str!("../polyfills/bootstrap-parts/part-15.js"),
    include_str!("../polyfills/bootstrap-parts/part-16.js"),
    include_str!("../polyfills/bootstrap-parts/part-17.js"),
    include_str!("../polyfills/bootstrap-parts/part-18.js"),
    include_str!("../polyfills/bootstrap-parts/part-19.js"),
    include_str!("../polyfills/bootstrap-parts/part-20.js"),
    include_str!("../polyfills/bootstrap-parts/part-21.js"),
];
static MKDTEMP_SEQUENCE: AtomicUsize = AtomicUsize::new(0);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut raw_args: Vec<String> = env::args().skip(1).collect();
    raw_args.retain(|arg| arg != "--experimental-stream-iter");
    let mut args = raw_args.into_iter();
    let mode = args.next();
    if mode.as_deref() == Some("--help") || mode.as_deref() == Some("-h") {
        println!("quench-node [--stage N|--test-dir DIR|--reuse-dir DIR|-e CODE|SCRIPT]");
        println!("  --reuse-dir reuses one rquickjs runtime with isolated contexts per script");
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
    let source = match mode.as_deref() {
        Some("-e") | Some("--eval") => args.next().unwrap_or_default(),
        Some(path) => fs::read_to_string(path)?,
        None => String::new(),
    };
    run_source(&source)
}

fn run_source(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::new()?;
    run_source_with_runtime(source, &runtime)
}

fn run_source_with_runtime(
    source: &str,
    runtime: &Runtime,
) -> Result<(), Box<dyn std::error::Error>> {
    let context = Context::full(runtime)?;
    run_host_context!(context, source)?;
    Ok(())
}

fn run_directory(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
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
            match run_source(&source) {
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
        Err("Node test harness found no JavaScript tests".into())
    } else if failed == 0 {
        Ok(())
    } else {
        Err("Node test harness failures".into())
    }
}

fn run_directory_reuse(dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Runtime::new()?;
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
            match run_source_with_runtime(&source, &runtime) {
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
        Err("Node reusable harness found no JavaScript tests".into())
    } else if failed == 0 {
        Ok(())
    } else {
        Err("Node reusable harness failures".into())
    }
}
