//! Strict images: the entry evaluated at build time (`prefix`), and an executable that keeps
//! only what the evaluation left. A program that reads no dynamic input leaves its effects
//! alone, which an image of a few kilobytes replays with no engine linked.
use super::prefix::Residual;
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};

/// The compile worker option that evaluates an entry at build time.
pub(super) const EVALUATION_WORKER_OPTION: &str = "--evaluation-worker";

/// The compile worker option that resumes an entry from the wait a build-time evaluation saved.
pub(super) const RESUMPTION_WORKER_OPTION: &str = "--resumption-worker";

/// The worker: resumes `entry` from `rest`, the runtime its build-time evaluation saved at its
/// first wait for input (`Residual::Waiting`), with `arguments` as the process's own, and exits
/// with the run's status. It checks a saved wait against the entry run from its start, before
/// an image keeps one.
pub(super) fn resume(entry: &Path, rest: &Path, arguments: Vec<String>) -> Result<(), String> {
    let rest = fs::read(rest).map_err(|error| format!("{}: {error}", rest.display()))?;
    let input = crate::shared_run::SharedInput::File {
        path: entry.to_owned(),
        exec_argv: Vec::new(),
        goal: crate::shared_run::EntryGoal::Node,
    };
    let mut argv = vec!["quench".to_owned(), entry.to_string_lossy().into_owned()];
    argv.extend(arguments);
    match crate::shared_run::resume_shared(input, argv, rest)? {
        crate::shared_run::SharedCompletion::GuestExit { code } => std::process::exit(code),
        _ => Ok(()),
    }
}

/// The worker: evaluates `entry` at build time under `assumptions` (`QUESTION=0|1`, a
/// question by its key and its answer) and writes what it left to `journal`.
pub(super) fn evaluate(entry: &Path, journal: &Path, assumptions: &[String]) -> Result<(), String> {
    super::prefix::begin(parse_assumptions(assumptions)?);
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
        (_, Residual::Question(question)) => json!({"status": "question", "key": question.key()}),
        (_, Residual::Waiting(effects, rest, _)) => {
            let path = journal.with_extension("rest");
            fs::write(&path, &rest).map_err(|error| format!("{}: {error}", path.display()))?;
            json!({"status": "waiting", "effects": effects, "rest": path, "rest_bytes": rest.len()})
        }
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

/// The input a run waits for, beside a run that completes under another answer.
const WAITED_INPUT: &str = "standard input, after it waits for it";

/// The answers a worker's `QUESTION=0|1` arguments assume.
pub(super) fn parse_assumptions(assumptions: &[String]) -> Result<Answers, String> {
    assumptions.iter().map(|assumption| {
        let (key, value) = assumption.rsplit_once('=').ok_or("evaluation assumption must be QUESTION=0|1")?;
        Ok((super::prefix::Question::of_key(key)?, value == "1"))
    }).collect()
}

/// The most questions whose answers one image specializes on.
const MOST_QUESTIONS: usize = 3;

/// One specialization of the program: the answers it assumes, its effects in order and its
/// exit status.
struct Variant {
    answers: Answers,
    /// Whether the run reached a wait for input instead of completing.
    waits: bool,
    effects: Vec<(i32, Vec<u8>)>,
    exit: i32,
}

/// What a strict compile made: a strict image; nothing, because the build-time run waits for
/// input before reading any, which an image linked with support resumes (`Waits`, `aot::cli`);
/// or nothing, because a build-time run read the named dynamic input, which strict images do
/// not yet keep code for, and the compile links the program's support variant.
pub(super) enum Outcome {
    Image,
    /// The answers each waiting run assumed, one run per set.
    Waits(Vec<Answers>),
    Dynamic,
}

/// The answers a build-time run assumes, each a question and its answer.
pub(super) type Answers = Vec<(super::prefix::Question, bool)>;

/// Compiles `entry` to a strict image at `output`, in the build directory `work`; `report`
/// gains what the evaluations left.
pub(super) fn compile(entry: &Path, output: &Path, work: &Path, report: &mut Value) -> Result<Outcome, String> {
    let mut variants = Vec::new();
    if let Some(input) = specialize(entry, work, Vec::new(), &mut variants)? {
        report["strict"] = json!({"status": "dynamic_input", "input": input});
        return Ok(Outcome::Dynamic);
    }
    // Runs that wait for input resume in an image with support; one that waits beside one that
    // completes would need both kinds of image.
    if variants.iter().all(|variant| variant.waits) {
        report["strict"] = json!({"status": "waits_for_input", "runs": variants.iter().map(|variant|
            variant.answers.iter().map(|(question, answer)| (question.key(), *answer)).collect::<Vec<_>>()).collect::<Vec<_>>()});
        return Ok(Outcome::Waits(variants.into_iter().map(|variant| variant.answers).collect()));
    }
    if variants.iter().any(|variant| variant.waits) {
        report["strict"] = json!({"status": "dynamic_input", "input": WAITED_INPUT});
        return Ok(Outcome::Dynamic);
    }
    let source = work.join("strict_image.rs");
    fs::write(&source, replay_source(&variants)).map_err(|error| error.to_string())?;
    let mut rustc = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    rustc.args(["--edition", "2024", "--crate-type", "bin", "-C", "panic=abort", "-C", "opt-level=s", "-C", "strip=symbols", "-C", "link-arg=-lc"])
        .arg("-o").arg(output).arg(&source);
    quench_runtime::native_link::checked_output(&mut rustc, "strict image link")?;
    report["strict"] = json!({"status": "image", "evaluation": variants.iter().map(|variant| json!({
        "answers": variant.answers.iter().map(|(question, answer)| (question.key(), *answer)).collect::<Vec<_>>(), "writes": variant.effects.len(),
        "bytes": variant.effects.iter().map(|(_, bytes)| bytes.len()).sum::<usize>(), "exit": variant.exit,
    })).collect::<Vec<_>>()});
    report["engine_linked"] = json!(false);
    Ok(Outcome::Image)
}

/// Evaluates `entry` under `answers`, and once per answer of each question the run asks,
/// adding each run that completes or waits for input to `variants`; or the first dynamic input
/// a run read.
fn specialize(entry: &Path, work: &Path, answers: Answers, variants: &mut Vec<Variant>) -> Result<Option<String>, String> {
    let journal = work.join(format!("evaluation-{}.json", variants.len()));
    let mut worker = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    worker.args(["compile", EVALUATION_WORKER_OPTION]).arg(entry).arg(&journal)
        .args(answers.iter().map(|(question, answer)| format!("{}={}", question.key(), u8::from(*answer))));
    crate::host::startup_snapshot::neutralize_native_bootstrap_environment(&mut worker);
    quench_runtime::native_link::checked_output(&mut worker, "build-time evaluation")?;
    let left: Value = serde_json::from_slice(&fs::read(&journal).map_err(|error| error.to_string())?)
        .map_err(|error| format!("build-time evaluation journal: {error}"))?;
    match left["status"].as_str() {
        Some("effects") => {
            let effects = serde_json::from_value(left["effects"].clone()).map_err(|error| error.to_string())?;
            variants.push(Variant { answers, waits: false, effects, exit: left["exit"].as_i64().unwrap_or(0) as i32 });
            Ok(None)
        }
        Some("question") if answers.len() < MOST_QUESTIONS => {
            let question = super::prefix::Question::of_key(left["key"].as_str().ok_or("build-time evaluation journal: question without its key")?)?;
            for answer in [false, true] {
                let mut assumed = answers.clone();
                assumed.push((question.clone(), answer));
                if let Some(input) = specialize(entry, work, assumed, variants)? {
                    return Ok(Some(input));
                }
            }
            Ok(None)
        }
        Some("question") => Ok(Some(format!("more than {MOST_QUESTIONS} yes-or-no inputs ({})", left["key"].as_str().unwrap_or("")))),
        Some("waiting") => {
            variants.push(Variant { answers, waits: true, effects: Vec::new(), exit: 0 });
            Ok(None)
        }
        Some("dynamic") => Ok(Some(left["input"].as_str().unwrap_or("an unnamed input").to_owned())),
        _ => Err(format!("build-time evaluation journal: unknown status {}", left["status"])),
    }
}

/// The image of the program's `variants`: as it starts it asks the questions the variants
/// assume answers to (`asked`), and the variant whose answers hold makes its writes in
/// order to their descriptors, as the program's stream writes made them (a closed or failing
/// descriptor is ignored, as Node's stdio writes ignore it, with SIGPIPE ignored as Node
/// ignores it), then exits with its status.
fn replay_source(variants: &[Variant]) -> String {
    let mut source = String::from(
        "#![no_std]\n#![no_main]\n\
         unsafe extern \"C\" { fn write(fd: i32, buf: *const u8, n: usize) -> isize; fn signal(signal: i32, handler: usize) -> usize; fn isatty(fd: i32) -> i32; fn getenv(name: *const u8) -> *const u8; }\n\
         #[panic_handler] fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }\n\
         fn put(fd: i32, bytes: &[u8]) { let mut at = 0; while at < bytes.len() { let n = unsafe { write(fd, bytes.as_ptr().add(at), bytes.len() - at) }; if n <= 0 { return; } at += n as usize; } }\n\
         fn terminal(fd: i32) -> bool { unsafe { isatty(fd) == 1 } }\n\
         fn environment(name: &[u8], value: &[u8]) -> bool { let at = unsafe { getenv(name.as_ptr()) }; if at.is_null() { return false; } let mut index = 0; loop { let byte = unsafe { *at.add(index) }; if index == value.len() { return byte == 0; } if byte != value[index] { return false; } index += 1; } }\n",
    );
    for (number, variant) in variants.iter().enumerate() {
        for (index, (_, bytes)) in variant.effects.iter().enumerate() {
            source.push_str(&format!("static V{number}E{index}: [u8; {}] = {:?};\n", bytes.len(), bytes));
        }
    }
    source.push_str("#[unsafe(no_mangle)] pub extern \"C\" fn main() -> i32 {\n    unsafe { signal(13, 1) };\n");
    for (number, variant) in variants.iter().enumerate() {
        let guard = variant.answers.iter()
            .map(|(question, answer)| format!("{}{}", if *answer { "" } else { "!" }, asked(question)))
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

/// The image's expression that asks `question` as it starts.
fn asked(question: &super::prefix::Question) -> String {
    match question {
        super::prefix::Question::Terminal(fd) => format!("terminal({fd})"),
        super::prefix::Question::Environment(name, value) => {
            let mut name = name.as_bytes().to_vec();
            name.push(0);
            format!("environment(&{name:?}, &{:?})", value.as_bytes())
        }
    }
}
