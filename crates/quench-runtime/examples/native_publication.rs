//! Compare the two publications of the call-threaded native graph on one
//! script: Cranelift per function, and copy-and-patch stencils.
//!
//! ```text
//! cargo run --release -p quench-runtime --features aot --example native_publication -- SCRIPT [ROUNDS]
//! ```
//!
//! Each round compiles and runs the script once per publication, plus the
//! interpreter as a control, in an order that alternates between rounds.
//! Reported per publication: median compile time, executable anonymous
//! mapping bytes the compile added, and median execution time.

use quench_runtime::{
    Engine, ExecutionMode, NativeJsBackend, NativeJsCodegenOptions, NativeJsProgram, ResidualProgram, Runtime, SystemHost,
};
use std::time::{Duration, Instant};

const DEFAULT_ROUNDS: usize = 11;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Subject {
    Interpreter,
    Cranelift,
    Stencil,
}

#[derive(Default)]
struct Samples {
    compile: Vec<Duration>,
    execute: Vec<Duration>,
    mapped: Vec<usize>,
}

/// Executable anonymous mappings of this process, in bytes.
fn executable_anonymous_bytes() -> usize {
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    maps.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let range = fields.next()?;
            let permissions = fields.next()?;
            let path = fields.nth(3);
            (permissions.contains('x') && path.is_none()).then_some(range)
        })
        .filter_map(|range| {
            let (start, end) = range.split_once('-')?;
            Some(usize::from_str_radix(end, 16).ok()? - usize::from_str_radix(start, 16).ok()?)
        })
        .sum()
}

fn run(subject: Subject, program: &ResidualProgram, samples: &mut Samples) -> Result<(), String> {
    let mut runtime = Runtime::new(SystemHost);
    if subject == Subject::Interpreter {
        let started = Instant::now();
        runtime.execute(program).map_err(|error| format!("{error:?}"))?;
        samples.execute.push(started.elapsed());
        return Ok(());
    }
    let backend = if subject == Subject::Stencil { NativeJsBackend::Stencil } else { NativeJsBackend::Cranelift };
    let options = NativeJsCodegenOptions { backend, ..Default::default() };
    let mapped_before = executable_anonymous_bytes();
    let started = Instant::now();
    let native = NativeJsProgram::<SystemHost>::compile_eager_with_options(program.clone(), options)?;
    samples.compile.push(started.elapsed());
    samples.mapped.push(executable_anonymous_bytes().saturating_sub(mapped_before));
    runtime.set_execution_mode(ExecutionMode::NativeOnly);
    let started = Instant::now();
    runtime.execute_native(&native).map_err(|error| format!("{error:?}"))?;
    samples.execute.push(started.elapsed());
    if runtime.native_execution_rejections() != 0 {
        return Err(format!("{subject:?}: native execution rejected an operation"));
    }
    Ok(())
}

fn median<T: Copy + Ord>(values: &[T]) -> Option<T> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.get(sorted.len() / 2).copied()
}

fn measure() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: native_publication SCRIPT [ROUNDS]")?;
    let rounds = args.next().map(|rounds| rounds.parse::<usize>().map_err(|error| error.to_string())).transpose()?.unwrap_or(DEFAULT_ROUNDS);
    let source = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
    let program = Engine::specialize_unspecialized(&source, &path).map_err(|errors| format!("{errors:?}"))?;
    let order = [Subject::Interpreter, Subject::Cranelift, Subject::Stencil];
    let mut samples: [Samples; 3] = Default::default();
    for round in 0..rounds {
        for offset in 0..order.len() {
            let index = (round + offset) % order.len();
            run(order[index], &program, &mut samples[index])?;
        }
    }
    println!("script {path}: {} functions, {rounds} rounds", program.function_count());
    println!("{:<12} {:>12} {:>14} {:>12}", "subject", "compile ms", "mapped KiB", "execute ms");
    for (subject, samples) in order.iter().zip(&samples) {
        let compile = median(&samples.compile).map_or_else(|| "-".into(), |value| format!("{:.3}", value.as_secs_f64() * 1e3));
        let mapped = median(&samples.mapped).map_or_else(|| "-".into(), |value| format!("{:.1}", value as f64 / 1024.0));
        let execute = median(&samples.execute).map_or_else(|| "-".into(), |value| format!("{:.3}", value.as_secs_f64() * 1e3));
        println!("{:<12} {compile:>12} {mapped:>14} {execute:>12}", format!("{subject:?}"));
    }
    Ok(())
}

fn main() {
    let worker = std::thread::Builder::new()
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(measure)
        .expect("measurement worker");
    if let Err(error) = worker.join().expect("measurement worker panicked") {
        eprintln!("native_publication: {error}");
        std::process::exit(1);
    }
}
