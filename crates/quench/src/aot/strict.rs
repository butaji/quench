//! Strict images: the entry evaluated at build time (`prefix`), and an executable that keeps
//! only what the evaluation left. A program that reads no dynamic input leaves its effects
//! alone, which an image of a few kilobytes replays with no engine linked.
use super::prefix::Residual;
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

/// The compile worker option that evaluates an entry at build time.
pub(super) const EVALUATION_WORKER_OPTION: &str = "--evaluation-worker";

/// The worker: evaluates `entry` at build time and writes what it left to `journal`.
pub(super) fn evaluate(entry: &Path, journal: &Path) -> Result<(), String> {
    super::prefix::begin();
    let input = crate::shared_run::SharedInput::File {
        path: entry.to_owned(),
        exec_argv: Vec::new(),
        goal: crate::shared_run::EntryGoal::Node,
    };
    let argv = vec!["quench".to_owned(), entry.to_string_lossy().into_owned()];
    let completion = crate::shared_run::execute_shared(input, argv);
    let dynamic = |input: String| json!({"status": "dynamic", "input": input});
    let report = match (completion, super::prefix::finish()) {
        (_, Residual::Dynamic(input)) => dynamic(input),
        (Err(error), _) => dynamic(format!("an uncaught error ({error})")),
        (Ok(crate::shared_run::SharedCompletion::PendingTopLevelAwait), _) => dynamic("a top-level await that never settles".into()),
        (Ok(completion), Residual::Effects(effects)) => {
            let status = match completion {
                crate::shared_run::SharedCompletion::GuestExit { code } => code,
                _ => 0,
            };
            json!({"status": "effects", "exit": status, "effects": effects})
        }
    };
    fs::write(journal, report.to_string()).map_err(|error| format!("{}: {error}", journal.display()))
}

/// Compiles `entry` to a strict image at `output`, in the build directory `work`; `report`
/// gains what the evaluation left.
pub(super) fn compile(entry: &Path, output: &Path, work: &Path, report: &mut Value) -> Result<(), String> {
    let journal = work.join("evaluation.json");
    let mut worker = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    worker.args(["compile", EVALUATION_WORKER_OPTION]).arg(entry).arg(&journal);
    crate::host::startup_snapshot::neutralize_native_bootstrap_environment(&mut worker);
    quench_runtime::native_link::checked_output(&mut worker, "build-time evaluation")?;
    let left: Value = serde_json::from_slice(&fs::read(&journal).map_err(|error| error.to_string())?)
        .map_err(|error| format!("build-time evaluation journal: {error}"))?;
    if left["status"] != "effects" {
        report["evaluation"] = left.clone();
        return Err(format!(
            "strict AOT: {} reads {} at build time, and strict images do not yet keep code for dynamic inputs",
            entry.display(),
            left["input"].as_str().unwrap_or("a dynamic input"),
        ));
    }
    let effects: Vec<(i32, Vec<u8>)> = serde_json::from_value(left["effects"].clone()).map_err(|error| error.to_string())?;
    let exit = left["exit"].as_i64().unwrap_or(0) as i32;
    let source = work.join("strict_image.rs");
    fs::write(&source, replay_source(&effects, exit)).map_err(|error| error.to_string())?;
    let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    rustc.args(["--edition", "2024", "--crate-type", "bin", "-C", "panic=abort", "-C", "opt-level=s", "-C", "strip=symbols", "-C", "link-arg=-lc"])
        .arg("-o").arg(output).arg(&source);
    quench_runtime::native_link::checked_output(&mut rustc, "strict image link")?;
    report["evaluation"] = json!({"status": "effects", "writes": effects.len(), "bytes": effects.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(), "exit": exit});
    report["engine_linked"] = json!(false);
    Ok(())
}

/// The image of a program whose whole run is `effects` then exit status `exit`: each write
/// made in order to its descriptor, as the program's stream writes made them (a closed or
/// failing descriptor is ignored, as Node's stdio writes ignore it, with SIGPIPE ignored as
/// Node ignores it).
fn replay_source(effects: &[(i32, Vec<u8>)], exit: i32) -> String {
    let mut source = String::from(
        "#![no_std]\n#![no_main]\n\
         unsafe extern \"C\" { fn write(fd: i32, buf: *const u8, n: usize) -> isize; fn signal(signal: i32, handler: usize) -> usize; }\n\
         #[panic_handler] fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }\n\
         fn put(fd: i32, bytes: &[u8]) { let mut at = 0; while at < bytes.len() { let n = unsafe { write(fd, bytes.as_ptr().add(at), bytes.len() - at) }; if n <= 0 { return; } at += n as usize; } }\n",
    );
    for (index, (_, bytes)) in effects.iter().enumerate() {
        source.push_str(&format!("static E{index}: [u8; {}] = {:?};\n", bytes.len(), bytes));
    }
    source.push_str("#[unsafe(no_mangle)] pub extern \"C\" fn main() -> i32 {\n    unsafe { signal(13, 1) };\n");
    for (index, (fd, _)) in effects.iter().enumerate() {
        source.push_str(&format!("    put({fd}, &E{index});\n"));
    }
    source.push_str(&format!("    {exit}\n}}\n"));
    source
}
