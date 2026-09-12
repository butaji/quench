use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

const RUNNER: &str = r#"
let __quenchBenchSucceeded = true;
const __quenchBenchPrint = typeof console !== "undefined" && typeof console.log === "function"
  ? console.log.bind(console)
  : print;
BenchmarkSuite.RunSuites({
  NotifyResult(name, result) { __quenchBenchPrint("__quenchBenchResult: " + name + ": " + result); },
  NotifyError(name, error) { __quenchBenchSucceeded = false; __quenchBenchPrint("__quenchBenchError: " + name + ": " + error); },
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
struct FixtureRecord {
    json: String,
    source: String,
    valid: bool,
    node_score: Option<f64>,
    bun_score: Option<f64>,
    quench_score: Option<f64>,
}

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
    let mut output: Option<PathBuf> = None;
    if env::var_os("QUENCH_EXEC_TRACE").is_some() {
        usage("scored runs must not inherit QUENCH_EXEC_TRACE; use a diagnostic harness");
    }
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
            "--out" => {
                output = Some(PathBuf::from(
                    a.next().unwrap_or_else(|| usage("missing --out path")),
                ))
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
    let mut records = Vec::with_capacity(fsx.len());
    for f in fsx {
        let x = materialize(&f);
        let mut n = Vec::with_capacity(runs);
        let mut b = Vec::with_capacity(runs);
        let mut q = Vec::with_capacity(runs);
        let mut fixture_complete = true;
        for i in 0..runs {
            // Alternate the complete engine order so thermal drift and host
            // scheduling do not consistently favor one artifact.
            let order = if i % 2 == 0 {
                [(&node, 0u8), (&bun, 1), (&quench, 2)]
            } else {
                [(&quench, 2u8), (&bun, 1), (&node, 0)]
            };
            for (engine, slot) in order {
                let sample = run(engine, &[], &x, timeout_ms);
                match slot {
                    0 => n.push(sample),
                    1 => b.push(sample),
                    _ => q.push(sample),
                }
            }
            if !n.last().is_some_and(Sample::valid)
                || !b.last().is_some_and(Sample::valid)
                || !q.last().is_some_and(Sample::valid)
            {
                // A failed round cannot become valid through repetition. Keep
                // the attempted samples, mark the fixture incomplete, and
                // continue with the remaining fixtures.
                fixture_complete = false;
                break;
            }
        }
        let e = n.iter().zip(&b).zip(&q).all(|((n, b), q)| {
            n.status == b.status
                && b.status == q.status
                && semantic_output(&n.stdout) == semantic_output(&b.stdout)
                && semantic_output(&b.stdout) == semantic_output(&q.stdout)
        });
        // Scores are intentionally engine-dependent; output_equal is retained
        // as evidence, while validity is based on successful, scored runs.
        let valid = fixture_complete && n.iter().chain(&b).chain(&q).all(Sample::valid);
        let (nw, nr) = summary(&n);
        let (bw, br) = summary(&b);
        let (qw, qr) = summary(&q);
        let fixture_json = format!(
            "{{\"fixture\":{},\"runs\":{},\"valid\":{},\"output_equal\":{},\"node\":{},\"bun\":{},\"quench\":{}}}",
            json(&f.display().to_string()), runs, valid, e,
            engine_report(&n, nw, nr), engine_report(&b, bw, br),
            engine_report(&q, qw, qr)
        );
        println!("{fixture_json}");
        let _ = std::io::stdout().flush();
        // The materialized source is runner scratch, never benchmark state.
        // Remove it after all three bounded processes have reaped so a later
        // invocation cannot accidentally consume stale fixture contents.
        let _ = fs::remove_file(&x);
        if !valid {
            all_valid = false;
        }
        records.push(FixtureRecord {
            json: fixture_json,
            source: file_manifest(&f),
            valid,
            node_score: score_median(&n),
            bun_score: score_median(&b),
            quench_score: score_median(&q),
        });
    }
    if let Some(path) = output {
        write_report(&path, &records, runs, timeout_ms, &node, &bun, &quench);
    }
    if !all_valid {
        std::process::exit(1);
    }
}

impl Sample {
    fn valid(&self) -> bool {
        self.status == 0 && !self.timed_out && self.score.is_some_and(f64::is_finite)
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
fn score_median(samples: &[Sample]) -> Option<f64> {
    let mut values: Vec<_> = samples
        .iter()
        .filter_map(|s| s.score)
        .filter(|v| v.is_finite())
        .collect();
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied()
}
fn score_ci95(samples: &[Sample]) -> Option<f64> {
    let values: Vec<_> = samples
        .iter()
        .filter_map(|s| s.score)
        .filter(|v| v.is_finite())
        .collect();
    if values.len() < 2 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance =
        values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (values.len() - 1) as f64;
    Some(1.96 * variance.sqrt() / (values.len() as f64).sqrt())
}
fn engine_report(samples: &[Sample], wall: Option<u128>, rss: Option<u64>) -> String {
    format!(
        "{{\"wall_ns\":{},\"peak_rss_bytes\":{},\"score\":{},\"score_ci95\":{},\"samples\":{}}}",
        option_u128(wall),
        option_u64(rss),
        option_f64(score_median(samples)),
        option_f64(score_ci95(samples)),
        samples_json(samples)
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
fn samples_json(v: &[Sample]) -> String {
    format!("[{}]",v.iter().map(|s|format!("{{\"program\":{},\"status\":{},\"timed_out\":{},\"wall_ns\":{},\"peak_rss_bytes\":{},\"score\":{},\"instructions\":{},\"cycles\":{},\"page_faults\":{},\"page_reclaims\":{},\"involuntary_context_switches\":{},\"stdout\":{},\"stderr\":{}}}",json(&s.program),s.status,s.timed_out,s.wall_ns,option_u64(s.peak_rss_bytes),option_f64(s.score),option_u64(s.instructions),option_u64(s.cycles),option_u64(s.page_faults),option_u64(s.page_reclaims),option_u64(s.involuntary_context_switches),json(&s.stdout),json(&s.stderr))).collect::<Vec<_>>().join(","))
}
fn json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
fn option_u128(value: Option<u128>) -> String {
    value.map_or_else(|| "null".into(), |v| v.to_string())
}
fn option_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "null".into(), |v| v.to_string())
}
fn option_f64(value: Option<f64>) -> String {
    value.map_or_else(
        || "null".into(),
        |v| {
            if v.is_finite() {
                v.to_string()
            } else {
                "null".into()
            }
        },
    )
}
fn usage(s: &str) -> ! {
    eprintln!("{s}\nusage: quench-bench <fixture.js>|--all [--node PATH] [--bun PATH] [--quench PATH] [--runs N] [--timeout-ms N] [--out PATH]");
    std::process::exit(2)
}

fn semantic_output(stdout: &str) -> String {
    stdout
        .lines()
        .filter(|line| {
            !line.starts_with("Score: ")
                && *line != "----"
                && !line.starts_with("__quenchBenchResult: ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_report(
    path: &Path,
    records: &[FixtureRecord],
    runs: usize,
    timeout_ms: u64,
    node: &str,
    bun: &str,
    quench: &str,
) {
    let base = Path::new("quench-bench/js-engine-benchmark/v8-v7/base.js");
    let fixtures = records
        .iter()
        .map(|r| r.json.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let complete = records.iter().all(|r| r.valid);
    let aggregate = if complete {
        format!(
            "{{\"node\":{},\"bun\":{},\"quench\":{}}}",
            option_f64(geometric_mean(records.iter().filter_map(|r| r.node_score))),
            option_f64(geometric_mean(records.iter().filter_map(|r| r.bun_score))),
            option_f64(geometric_mean(
                records.iter().filter_map(|r| r.quench_score)
            ))
        )
    } else {
        "null".into()
    };
    let source_fixtures = records
        .iter()
        .map(|r| r.source.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let report = format!(
        "{{\"schema\":2,\"created_unix_ns\":{},\"runs\":{},\"timeout_ms\":{},\"git\":{},\"environment\":{},\"source\":{{\"base\":{},\"fixtures\":[{}]}},\"artifacts\":{{\"node\":{},\"bun\":{},\"quench\":{}}},\"fixtures\":[{}],\"complete\":{},\"aggregate_score\":{}}}",
        now_ns(), runs, timeout_ms, git_identity(), environment_manifest(), file_manifest(base), source_fixtures,
        artifact_manifest(node), artifact_manifest(bun), artifact_manifest(quench),
        fixtures, complete, aggregate
    );
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap_or_else(|e| panic!("refusing to overwrite evidence {}: {e}", path.display()));
    file.write_all(report.as_bytes())
        .expect("write evidence report");
}

fn geometric_mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<_> = values.filter(|v| v.is_finite() && *v > 0.0).collect();
    if values.is_empty() || values.len() != 8 {
        return None;
    }
    Some((values.iter().map(|v| v.ln()).sum::<f64>() / values.len() as f64).exp())
}

fn now_ns() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos())
}
fn git_identity() -> String {
    let rev = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let dirty = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .ok()
        .is_some_and(|o| !o.stdout.is_empty());
    format!("{{\"revision\":{},\"dirty\":{}}}", json(&rev), dirty)
}
fn environment_manifest() -> String {
    format!(
        "{{\"rustc\":{},\"host\":{}}}",
        command_text("rustc", &["-Vv"]),
        command_text("uname", &["-a"])
    )
}
fn command_text(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| json(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_else(|| "null".into())
}
fn artifact_manifest(path: &str) -> String {
    file_manifest(Path::new(path))
}
fn file_manifest(path: &Path) -> String {
    let metadata = fs::metadata(path).ok();
    let size = metadata.as_ref().map(|m| m.len());
    let modified = metadata
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos());
    let hash = sha256(path);
    format!(
        "{{\"path\":{},\"size\":{},\"modified_unix_ns\":{},\"sha256\":{}}}",
        json(&path.display().to_string()),
        option_u64(size),
        option_u128(modified),
        hash.map_or_else(|| "null".into(), |h| json(&h))
    )
}
fn sha256(path: &Path) -> Option<String> {
    let output = Command::new("shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{
        geometric_mean, option_u128, option_u64, score_ci95, semantic_output, summary, Sample,
    };

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

    #[test]
    fn semantic_output_ignores_engine_scores() {
        assert_eq!(semantic_output("foo: 1\n----\nScore: 12.5\n"), "foo: 1");
    }

    #[test]
    fn sample_validity_rejects_missing_or_unusable_scores() {
        let sample = |status, timed_out, score| Sample {
            program: "test".into(),
            status,
            timed_out,
            wall_ns: 1,
            peak_rss_bytes: None,
            score,
            instructions: None,
            cycles: None,
            page_faults: None,
            page_reclaims: None,
            involuntary_context_switches: None,
            stdout: String::new(),
            stderr: String::new(),
        };
        assert!(sample(0, false, Some(1.0)).valid());
        assert!(!sample(1, false, Some(1.0)).valid());
        assert!(!sample(0, true, Some(1.0)).valid());
        assert!(!sample(0, false, None).valid());
        assert!(!sample(0, false, Some(f64::NAN)).valid());
    }

    #[test]
    fn confidence_and_geometric_mean_require_real_samples() {
        let samples = (1..=3)
            .map(|score| Sample {
                program: "test".into(),
                status: 0,
                timed_out: false,
                wall_ns: 1,
                peak_rss_bytes: None,
                score: Some(score as f64),
                instructions: None,
                cycles: None,
                page_faults: None,
                page_reclaims: None,
                involuntary_context_switches: None,
                stdout: String::new(),
                stderr: String::new(),
            })
            .collect::<Vec<_>>();
        assert!(score_ci95(&samples).is_some_and(|v| v > 0.0));
        assert!(geometric_mean(std::iter::repeat(2.0).take(8))
            .is_some_and(|v| (v - 2.0).abs() < f64::EPSILON));
        assert!(geometric_mean(std::iter::repeat(2.0).take(7)).is_none());
    }
}
