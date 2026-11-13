//! `quench-stencil-mine [options] PATH...` ranks candidate supernodes for the stencil library.
use quench_stencil_mine::inputs::compile_paths;
use quench_stencil_mine::miner::mine_corpus;
use quench_stencil_mine::rank::{best_first, rank};
use quench_stencil_mine::recommend::{Budget, recommend};
use quench_stencil_mine::report::Report;
use quench_stencil_mine::variants::PlaceTable;
use quench_stencil_mine::weights::{TraceGrams, Weights, parse_census};
use std::path::PathBuf;

const DEFAULT_TOP: usize = 40;
/// Exit statuses: a command-line mistake, a failed run.
const USAGE_STATUS: i32 = 2;
const FAILURE_STATUS: i32 = 1;
const USAGE: &str = "usage: quench-stencil-mine [--census FILE] [--trace FILE] [--json FILE] [--top N] \
[--max-supernodes N] [--max-variants N] PATH...\n  PATH: JavaScript files or directories\n  \
--census: stderr of a QUENCH_TRACE=census=all run (profile-aggregate build)\n  \
--trace: opcode trace of a profile-trace build (4-byte little-endian opcodes)";

#[derive(Default)]
struct Args {
    paths: Vec<PathBuf>,
    census: Option<PathBuf>,
    trace: Option<PathBuf>,
    json: Option<PathBuf>,
    top: Option<usize>,
    supernodes: Option<usize>,
    variants: Option<u64>,
}

impl Args {
    /// Applies the option `flag`, taking its value from `args`.
    fn set(&mut self, flag: &str, args: &mut impl Iterator<Item = String>) -> Result<(), String> {
        let mut value = || args.next().ok_or(format!("{flag} needs a value"));
        match flag {
            "--census" => self.census = Some(value()?.into()),
            "--trace" => self.trace = Some(value()?.into()),
            "--json" => self.json = Some(value()?.into()),
            "--top" => self.top = Some(number(flag, &value()?)?),
            "--max-supernodes" => self.supernodes = Some(number(flag, &value()?)?),
            "--max-variants" => self.variants = Some(number(flag, &value()?)?),
            _ => return Err(format!("unknown option {flag}\n{USAGE}")),
        }
        Ok(())
    }
}

fn number<T: std::str::FromStr>(flag: &str, text: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    text.parse().map_err(|error| format!("{flag}: {error}"))
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut parsed = Args::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Err(USAGE.to_string()),
            flag if flag.starts_with("--") => parsed.set(flag, &mut args)?,
            path => parsed.paths.push(path.into()),
        }
    }
    if parsed.paths.is_empty() {
        return Err(USAGE.to_string());
    }
    Ok(parsed)
}

fn run(args: Args) -> Result<(), String> {
    let compiled = compile_paths(&args.paths);
    for failure in &compiled.failures {
        eprintln!("skipped (does not compile): {}", failure.display());
    }
    let census = args.census.map(|path| std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))).transpose()?;
    let trace = args.trace.map(|path| std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))).transpose()?;
    let weights = Weights::build(&compiled.functions, census.as_deref().map(parse_census), trace.as_deref().map(TraceGrams::from_bytes));
    let candidates = mine_corpus(&compiled.functions, &weights);
    let places = PlaceTable::new();
    let ranked = rank(&candidates, &weights, &places);
    let defaults = Budget::default();
    let budget = Budget { supernodes: args.supernodes.unwrap_or(defaults.supernodes), variants: args.variants.unwrap_or(defaults.variants) };
    let recommended = recommend(&candidates, &ranked, weights.total, &budget);
    let sorted = best_first(&ranked);
    let report = Report {
        weight_source: weights.source.name(),
        total_dynamic: weights.total,
        static_instructions: compiled.functions.iter().map(|f| f.instructions.len()).sum(),
        library_rows: places.library_rows,
        budget: &budget,
        ranked: &sorted,
        recommended: &recommended,
    };
    let top = args.top.unwrap_or(DEFAULT_TOP);
    print!("{}", report.text(top));
    if let Some(path) = args.json {
        std::fs::write(&path, report.json(top).to_string()).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

fn main() {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(USAGE_STATUS);
        }
    };
    // The compiler recurses deeply on nested source; run on the runtime's worker stack size.
    let worker = std::thread::Builder::new().stack_size(quench_runtime::WORKER_STACK_SIZE).spawn(move || run(args));
    match worker.map(|handle| handle.join()) {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(message))) => {
            eprintln!("{message}");
            std::process::exit(FAILURE_STATUS);
        }
        _ => std::process::exit(FAILURE_STATUS),
    }
}
