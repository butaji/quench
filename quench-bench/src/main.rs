use std::{
    env, fs,
    path::PathBuf,
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const RUNNER: &str = r#"
let __quenchBenchSucceeded = true;
const __quenchBenchPrint = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
BenchmarkSuite.RunSuites({
  NotifyResult(name, result) { __quenchBenchPrint(name + ": " + result); },
  NotifyError(name, error) { __quenchBenchSucceeded = false; __quenchBenchPrint(name + ": " + error); },
  NotifyScore(score) {
    if (__quenchBenchSucceeded) {
      __quenchBenchPrint("----");
      __quenchBenchPrint("Score: " + score);
    }
  },
});
"#;
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

#[derive(Debug)]
struct Sample {
    program: String,
    status: i32,
    timed_out: bool,
    wall_ns: u128,
    peak_rss_bytes: Option<u64>,
    score: Option<f64>,
    instructions: Option<u64>,
    cycles: Option<u64>,
    page_faults: Option<u64>,
    page_reclaims: Option<u64>,
    involuntary_context_switches: Option<u64>,
    stdout: String,
    stderr: String,
}
fn main() {
    let mut a = env::args().skip(1);
    let first = a
        .next()
        .unwrap_or_else(|| usage("missing fixture or --all"));
    let mut node = "node".into();
    let mut bun = "bun".into();
    let mut quench = "target/bench-throughput/quench-node".into();
    let mut runs = 1usize;
    // Every engine invocation is bounded unless the caller explicitly opts
    // into a different positive duration.  The suite still records all
    // fixtures after a timeout so one stale workload cannot hide the rest.
    let mut timeout_ms = DEFAULT_TIMEOUT_MS;
    while let Some(x) = a.next() {
        match x.as_str() {
            "--node" => node = a.next().unwrap_or_else(|| usage("missing --node path")),
            "--bun" => bun = a.next().unwrap_or_else(|| usage("missing --bun path")),
            "--quench" => quench = a.next().unwrap_or_else(|| usage("missing --quench path")),
            "--runs" => {
                runs = a
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|v| *v > 0)
                    .unwrap_or_else(|| usage("invalid --runs"))
            }
            "--timeout-ms" => {
                timeout_ms = a
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|v| *v > 0)
                    .unwrap_or_else(|| usage("invalid --timeout-ms"))
            }
            _ => usage("unknown argument"),
        }
    }
    let fsx = if first == "--all" {
        let mut v: Vec<_> = fs::read_dir("quench-bench/js-engine-benchmark/v8-v7")
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "js")
                    && p.file_name()
                        .is_some_and(|n| n != "base.js" && n != "run.js")
            })
            .collect();
        v.sort();
        v
    } else {
        vec![PathBuf::from(first)]
    };
    let mut all_valid = true;
    for f in fsx {
        let x = materialize(&f);
        let n = (0..runs)
            .map(|_| run(&node, &[], &x, timeout_ms))
            .collect::<Vec<_>>();
        let b = (0..runs)
            .map(|_| run(&bun, &[], &x, timeout_ms))
            .collect::<Vec<_>>();
        let q = (0..runs)
            .map(|_| run(&quench, &[], &x, timeout_ms))
            .collect::<Vec<_>>();
        let e = n.iter().zip(&b).zip(&q).all(|((n, b), q)| {
            n.status == b.status
                && b.status == q.status
                && n.stdout == b.stdout
                && b.stdout == q.stdout
        });
        // Scores are intentionally engine-dependent; output_equal is retained
        // as evidence, while validity is based on successful, scored runs.
        let valid = n.iter().chain(&b).chain(&q).all(Sample::valid);
        let (nw, nr) = summary(&n);
        let (bw, br) = summary(&b);
        let (qw, qr) = summary(&q);
        println!(
            "{{\"fixture\":{},\"runs\":{},\"valid\":{},\"output_equal\":{},\"node\":{{\"wall_ns\":{},\"peak_rss_bytes\":{},\"samples\":{}}},\"bun\":{{\"wall_ns\":{},\"peak_rss_bytes\":{},\"samples\":{}}},\"quench\":{{\"wall_ns\":{},\"peak_rss_bytes\":{},\"samples\":{}}}}}",
            json(&f.display().to_string()), runs, valid, e,
            option_u128(nw), option_u64(nr), samples(&n),
            option_u128(bw), option_u64(br), samples(&b),
            option_u128(qw), option_u64(qr), samples(&q)
        );
        // The materialized source is runner scratch, never benchmark state.
        // Remove it after all three bounded processes have reaped so a later
        // invocation cannot accidentally consume stale fixture contents.
        let _ = fs::remove_file(&x);
        if !valid {
            all_valid = false;
        }
    }
    if !all_valid {
        std::process::exit(1);
    }
}

impl Sample {
    fn valid(&self) -> bool {
        self.status == 0 && !self.timed_out && self.score.is_some()
    }
}
fn summary(samples: &[Sample]) -> (Option<u128>, Option<u64>) {
    let mut walls: Vec<_> = samples.iter().map(|s| s.wall_ns).collect();
    walls.sort_unstable();
    let mut rss: Vec<_> = samples.iter().filter_map(|s| s.peak_rss_bytes).collect();
    rss.sort_unstable();
    (
        walls.get(walls.len() / 2).copied(),
        rss.get(rss.len() / 2).copied(),
    )
}
fn materialize(f: &PathBuf) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let p = PathBuf::from("/tmp").join(format!(
        "quench-bench-{}-{}-{}",
        std::process::id(),
        nonce,
        f.file_name().unwrap().to_string_lossy()
    ));
    let base = fs::read("quench-bench/js-engine-benchmark/v8-v7/base.js").unwrap();
    let fixture = fs::read(f).unwrap();
    let mut source = Vec::with_capacity(base.len() + fixture.len() + RUNNER.len() + 2);
    source.extend_from_slice(&base);
    source.push(b'\n');
    source.extend_from_slice(&fixture);
    source.push(b'\n');
    source.extend_from_slice(RUNNER.as_bytes());
    fs::write(&p, source).unwrap();
    p
}
fn run(p: &str, args: &[String], s: &PathBuf, t: u64) -> Sample {
    let st = Instant::now();
    let seconds = format!("{:.3}", t as f64 / 1000.0);
    let mut command = Command::new("timeout");
    command
        .args([
            "--signal=TERM",
            "--kill-after=1",
            &seconds,
            "/usr/bin/time",
            "-l",
            p,
        ])
        .args(args)
        .arg(s);
    let o = command.output();
    let (status, stderr, stdout) = match o {
        Ok(o) => (
            o.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&o.stderr).into_owned(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        ),
        Err(e) => (-1, e.to_string(), String::new()),
    };
    let timed_out = matches!(status, 124 | 137);
    let score = stdout
        .lines()
        .find_map(|line| line.strip_prefix("Score: "))
        .and_then(|value| value.parse().ok());
    Sample {
        program: p.into(),
        status,
        timed_out,
        wall_ns: st.elapsed().as_nanos(),
        peak_rss_bytes: time_metric(&stderr, "maximum resident set size"),
        score,
        instructions: time_metric(&stderr, "instructions retired"),
        cycles: time_metric(&stderr, "cycles elapsed"),
        page_faults: time_metric(&stderr, "page faults"),
        page_reclaims: time_metric(&stderr, "page reclaims"),
        involuntary_context_switches: time_metric(&stderr, "involuntary context switches"),
        stdout,
        stderr,
    }
}

fn time_metric(stderr: &str, suffix: &str) -> Option<u64> {
    stderr.lines().find_map(|line| {
        line.trim()
            .strip_suffix(suffix)
            .and_then(|value| value.trim().parse().ok())
    })
}
fn samples(v: &[Sample]) -> String {
    format!("[{}]",v.iter().map(|s|format!("{{\"program\":{},\"status\":{},\"timed_out\":{},\"wall_ns\":{},\"peak_rss_bytes\":{},\"score\":{},\"instructions\":{},\"cycles\":{},\"page_faults\":{},\"page_reclaims\":{},\"involuntary_context_switches\":{},\"stdout\":{},\"stderr\":{}}}",json(&s.program),s.status,s.timed_out,s.wall_ns,option_u64(s.peak_rss_bytes),option_f64(s.score),option_u64(s.instructions),option_u64(s.cycles),option_u64(s.page_faults),option_u64(s.page_reclaims),option_u64(s.involuntary_context_switches),json(&s.stdout),json(&s.stderr))).collect::<Vec<_>>().join(","))
}
fn json(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}
fn option_u128(value: Option<u128>) -> String {
    value.map_or_else(|| "null".into(), |v| v.to_string())
}
fn option_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "null".into(), |v| v.to_string())
}
fn option_f64(value: Option<f64>) -> String {
    value.map_or_else(|| "null".into(), |v| v.to_string())
}
fn usage(s: &str) -> ! {
    eprintln!("{s}\nusage: quench-bench <fixture.js>|--all [--node PATH] [--bun PATH] [--quench PATH] [--runs N] [--timeout-ms N]");
    std::process::exit(2)
}

#[cfg(test)]
mod tests {
    use super::{option_u128, option_u64, summary, Sample};

    #[test]
    fn summary_uses_median_measurements() {
        let samples = vec![
            Sample {
                program: "test".into(),
                status: 0,
                timed_out: false,
                wall_ns: 30,
                peak_rss_bytes: Some(300),
                score: Some(1.0),
                instructions: None,
                cycles: None,
                page_faults: None,
                page_reclaims: None,
                involuntary_context_switches: None,
                stdout: String::new(),
                stderr: String::new(),
            },
            Sample {
                program: "test".into(),
                status: 0,
                timed_out: false,
                wall_ns: 10,
                peak_rss_bytes: Some(100),
                score: Some(1.0),
                instructions: None,
                cycles: None,
                page_faults: None,
                page_reclaims: None,
                involuntary_context_switches: None,
                stdout: String::new(),
                stderr: String::new(),
            },
            Sample {
                program: "test".into(),
                status: 0,
                timed_out: false,
                wall_ns: 20,
                peak_rss_bytes: Some(200),
                score: Some(1.0),
                instructions: None,
                cycles: None,
                page_faults: None,
                page_reclaims: None,
                involuntary_context_switches: None,
                stdout: String::new(),
                stderr: String::new(),
            },
        ];
        assert_eq!(summary(&samples), (Some(20), Some(200)));
    }
    #[test]
    fn optional_summary_fields_use_json_null() {
        assert_eq!(option_u128(None), "null");
        assert_eq!(option_u128(Some(42)), "42");
        assert_eq!(option_u64(None), "null");
        assert_eq!(option_u64(Some(7)), "7");
    }
}
