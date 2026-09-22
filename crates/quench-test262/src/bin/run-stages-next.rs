use std::{env, path::PathBuf, process::ExitCode};

use quench_test262::{
    discover_js_files, resolve_stages, HarnessCache, RuntimeNextHost, Test262Runner,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("FAIL: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    let from = args
        .next()
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|_| "usage: run-stages-next [from] [to]".to_string())?
        .unwrap_or(0);
    let to = args
        .next()
        .map(|value| value.parse::<u32>())
        .transpose()
        .map_err(|_| "usage: run-stages-next [from] [to]".to_string())?
        .unwrap_or(from);
    let filter = args.next();
    if args.next().is_some() || from > to {
        return Err("usage: run-stages-next [from] [to] [path-filter]".into());
    }
    let root = env::var_os("TEST262_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("tests/test262"));
    let stages = resolve_stages(&root)?;
    let mut runner = Test262Runner::new(RuntimeNextHost::default());
    for stage in stages
        .into_iter()
        .filter(|stage| stage.id >= from && stage.id <= to)
    {
        let files = discover_js_files(&stage.root)?;
        let files = filter.as_ref().map_or(files.clone(), |needle| {
            files
                .into_iter()
                .filter(|path| path.to_string_lossy().contains(needle))
                .collect()
        });
        let mut cache = HarnessCache::new(root.join("harness"));
        let report = runner.run_files_with_cache(files, &mut cache)?;
        println!(
            "next stage {:>3}: {} passed={} failed={} total={}",
            stage.id, stage.path, report.passed, report.failed, report.total
        );
        if report.failed != 0 {
            for (path, reason) in report.failures.iter().take(10) {
                eprintln!("  {}: {}", path.display(), reason);
            }
            return Err(format!("next stage {} failed", stage.id));
        }
    }
    Ok(())
}
