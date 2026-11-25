//! Strict images: the entry evaluated at build time (`prefix`), and an executable that keeps
//! only what the evaluation left. A program that reads no dynamic input leaves its effects
//! alone, which an image of a few kilobytes replays with no engine linked.
use super::prefix::Residual;
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

/// The compile worker option that evaluates an entry at build time.
pub(super) const EVALUATION_WORKER_OPTION: &str = "--evaluation-worker";

/// The worker: evaluates `entry` at build time under `assumptions` (`FD=0|1`: whether
/// descriptor FD is a terminal) and writes what it left to `journal`.
pub(super) fn evaluate(entry: &Path, journal: &Path, assumptions: &[String]) -> Result<(), String> {
    let terminals = assumptions.iter().map(|assumption| {
        let (fd, value) = assumption.split_once('=').ok_or("evaluation assumption must be FD=0|1")?;
        Ok((fd.parse::<i32>().map_err(|error| error.to_string())?, value == "1"))
    }).collect::<Result<Vec<_>, String>>()?;
    super::prefix::begin(terminals);
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
        (_, Residual::Terminal(fd)) => json!({"status": "terminal", "fd": fd}),
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

/// The most descriptors whose terminal answers one image specializes on.
const MOST_TERMINAL_CHOICES: usize = 3;

/// One specialization of the program: the terminal answers it assumes, its effects in order
/// and its exit status.
struct Variant {
    terminals: Vec<(i32, bool)>,
    effects: Vec<(i32, Vec<u8>)>,
    exit: i32,
}

/// Compiles `entry` to a strict image at `output`, in the build directory `work`; `report`
/// gains what the evaluations left.
pub(super) fn compile(entry: &Path, output: &Path, work: &Path, report: &mut Value) -> Result<(), String> {
    let mut variants = Vec::new();
    specialize(entry, work, Vec::new(), &mut variants)?;
    let source = work.join("strict_image.rs");
    fs::write(&source, replay_source(&variants)).map_err(|error| error.to_string())?;
    let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    rustc.args(["--edition", "2024", "--crate-type", "bin", "-C", "panic=abort", "-C", "opt-level=s", "-C", "strip=symbols", "-C", "link-arg=-lc"])
        .arg("-o").arg(output).arg(&source);
    quench_runtime::native_link::checked_output(&mut rustc, "strict image link")?;
    report["evaluation"] = json!(variants.iter().map(|variant| json!({
        "terminals": variant.terminals, "writes": variant.effects.len(),
        "bytes": variant.effects.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(), "exit": variant.exit,
    })).collect::<Vec<_>>());
    report["engine_linked"] = json!(false);
    Ok(())
}

/// Evaluates `entry` under `terminals`, and once per answer of each terminal question the
/// run asks, adding each completed run to `variants`.
fn specialize(entry: &Path, work: &Path, terminals: Vec<(i32, bool)>, variants: &mut Vec<Variant>) -> Result<(), String> {
    let journal = work.join(format!("evaluation-{}.json", variants.len()));
    let mut worker = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    worker.args(["compile", EVALUATION_WORKER_OPTION]).arg(entry).arg(&journal)
        .args(terminals.iter().map(|(fd, terminal)| format!("{fd}={}", u8::from(*terminal))));
    crate::host::startup_snapshot::neutralize_native_bootstrap_environment(&mut worker);
    quench_runtime::native_link::checked_output(&mut worker, "build-time evaluation")?;
    let left: Value = serde_json::from_slice(&fs::read(&journal).map_err(|error| error.to_string())?)
        .map_err(|error| format!("build-time evaluation journal: {error}"))?;
    match left["status"].as_str() {
        Some("effects") => {
            let effects = serde_json::from_value(left["effects"].clone()).map_err(|error| error.to_string())?;
            variants.push(Variant { terminals, effects, exit: left["exit"].as_i64().unwrap_or(0) as i32 });
            Ok(())
        }
        Some("terminal") if terminals.len() < MOST_TERMINAL_CHOICES => {
            let fd = left["fd"].as_i64().ok_or("build-time evaluation journal: terminal without its descriptor")? as i32;
            for answer in [false, true] {
                let mut assumed = terminals.clone();
                assumed.push((fd, answer));
                specialize(entry, work, assumed, variants)?;
            }
            Ok(())
        }
        _ => Err(format!(
            "strict AOT: {} reads {} at build time, and strict images do not yet keep code for dynamic inputs",
            entry.display(),
            left["input"].as_str().unwrap_or("whether more descriptors are terminals"),
        )),
    }
}

/// The image of the program's `variants`: as it starts it asks whether each descriptor the
/// variants assume is a terminal, and the variant whose answers hold makes its writes in
/// order to their descriptors, as the program's stream writes made them (a closed or failing
/// descriptor is ignored, as Node's stdio writes ignore it, with SIGPIPE ignored as Node
/// ignores it), then exits with its status.
fn replay_source(variants: &[Variant]) -> String {
    let mut source = String::from(
        "#![no_std]\n#![no_main]\n\
         unsafe extern \"C\" { fn write(fd: i32, buf: *const u8, n: usize) -> isize; fn signal(signal: i32, handler: usize) -> usize; fn isatty(fd: i32) -> i32; }\n\
         #[panic_handler] fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }\n\
         fn put(fd: i32, bytes: &[u8]) { let mut at = 0; while at < bytes.len() { let n = unsafe { write(fd, bytes.as_ptr().add(at), bytes.len() - at) }; if n <= 0 { return; } at += n as usize; } }\n\
         fn terminal(fd: i32) -> bool { unsafe { isatty(fd) == 1 } }\n",
    );
    for (number, variant) in variants.iter().enumerate() {
        for (index, (_, bytes)) in variant.effects.iter().enumerate() {
            source.push_str(&format!("static V{number}E{index}: [u8; {}] = {:?};\n", bytes.len(), bytes));
        }
    }
    source.push_str("#[unsafe(no_mangle)] pub extern \"C\" fn main() -> i32 {\n    unsafe { signal(13, 1) };\n");
    for (number, variant) in variants.iter().enumerate() {
        let guard = variant.terminals.iter()
            .map(|(fd, terminal)| format!("{}terminal({fd})", if *terminal { "" } else { "!" }))
            .collect::<Vec<_>>();
        let guard = if guard.is_empty() { "true".to_owned() } else { guard.join(" && ") };
        source.push_str(&format!("    if {guard} {{\n"));
        for (index, (fd, _)) in variant.effects.iter().enumerate() {
            source.push_str(&format!("        put({fd}, &V{number}E{index});\n"));
        }
        source.push_str(&format!("        return {};\n    }}\n", variant.exit));
    }
    source.push_str("    70\n}\n");
    source
}
