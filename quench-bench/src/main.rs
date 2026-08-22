use std::{env, fs, path::PathBuf, process::Command, time::Instant};
#[derive(Debug)]
struct Sample {
    program: String,
    status: i32,
    wall_ns: u128,
    peak_rss_bytes: Option<u64>,
    stdout: String,
    stderr: String,
}
fn main() {
    let mut a = env::args().skip(1);
    let first = a
        .next()
        .unwrap_or_else(|| usage("missing fixture or --all"));
    let mut node = "node".into();
    let mut quench = "target/release/quench-node".into();
    let mut runs = 1usize;
    let mut timeout_ms = 120_000u64;
    while let Some(x) = a.next() {
        match x.as_str() {
            "--node" => node = a.next().unwrap_or_else(|| usage("missing --node path")),
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
    for f in fsx {
        let x = materialize(&f);
        let n = (0..runs)
            .map(|_| run(&node, &[], &x, timeout_ms))
            .collect::<Vec<_>>();
        let q = (0..runs)
            .map(|_| run(&quench, &[], &x, timeout_ms))
            .collect::<Vec<_>>();
        let e = n
            .iter()
            .zip(&q)
            .all(|(n, q)| n.status == q.status && n.stdout == q.stdout);
        let (nw, nr) = summary(&n);
        let (qw, qr) = summary(&q);
        let wall_ratio = ratio_u128(nw, qw);
        let rss_ratio = ratio_u64(nr, qr);
        let rss_target = rss_ratio.parse::<f64>().is_ok_and(|ratio| ratio >= 10.0);
        println!("{{\"fixture\":{},\"runs\":{},\"output_equal\":{},\"wall_ratio\":{},\"rss_ratio\":{},\"rss_target_met\":{},\"node\":{},\"quench\":{}}}",json(&f.display().to_string()),runs,e,wall_ratio,rss_ratio,rss_target,samples(&n),samples(&q));
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
fn ratio_u128(node: Option<u128>, quench: Option<u128>) -> String {
    match (node, quench) {
        (Some(n), Some(q)) if q > 0 => format!("{:.4}", n as f64 / q as f64),
        _ => "null".into(),
    }
}
fn ratio_u64(node: Option<u64>, quench: Option<u64>) -> String {
    match (node, quench) {
        (Some(n), Some(q)) if q > 0 => format!("{:.4}", n as f64 / q as f64),
        _ => "null".into(),
    }
}
fn materialize(f: &PathBuf) -> PathBuf {
    let p = PathBuf::from("/tmp").join(format!(
        "quench-bench-{}",
        f.file_name().unwrap().to_string_lossy()
    ));
    let base = fs::read("quench-bench/js-engine-benchmark/v8-v7/base.js").unwrap();
    let fixture = fs::read(f).unwrap();
    let mut source = Vec::with_capacity(base.len() + 1 + fixture.len());
    source.extend_from_slice(&base);
    source.push(b'\n');
    source.extend_from_slice(&fixture);
    fs::write(&p, source).unwrap();
    p
}
fn run(p: &str, args: &[String], s: &PathBuf, t: u64) -> Sample {
    let st = Instant::now();
    let seconds = format!("{:.3}", t as f64 / 1000.0);
    let mut command = Command::new("/usr/bin/time");
    command
        .args(["-l", "timeout", "--signal=KILL", &seconds, p])
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
    Sample {
        program: p.into(),
        status,
        wall_ns: st.elapsed().as_nanos(),
        peak_rss_bytes: stderr.lines().find_map(|l| {
            l.trim()
                .strip_suffix(" maximum resident set size")
                .and_then(|v| v.trim().parse().ok())
        }),
        stdout,
        stderr,
    }
}
fn samples(v: &[Sample]) -> String {
    format!("[{}]",v.iter().map(|s|format!("{{\"program\":{},\"status\":{},\"wall_ns\":{},\"peak_rss_bytes\":{},\"stdout\":{},\"stderr\":{}}}",json(&s.program),s.status,s.wall_ns,s.peak_rss_bytes.map_or("null".into(),|x:u64|x.to_string()),json(&s.stdout),json(&s.stderr))).collect::<Vec<_>>().join(","))
}
fn json(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
    )
}
fn usage(s: &str) -> ! {
    eprintln!("{s}\nusage: quench-bench <fixture.js>|--all [--runs N] [--timeout-ms N] [--quench-arg ARG]");
    std::process::exit(2)
}

#[cfg(test)]
mod tests {
    use super::{summary, Sample};

    #[test]
    fn summary_uses_median_measurements() {
        let samples = vec![
            Sample {
                program: "test".into(),
                status: 0,
                wall_ns: 30,
                peak_rss_bytes: Some(300),
                stdout: String::new(),
                stderr: String::new(),
            },
            Sample {
                program: "test".into(),
                status: 0,
                wall_ns: 10,
                peak_rss_bytes: Some(100),
                stdout: String::new(),
                stderr: String::new(),
            },
            Sample {
                program: "test".into(),
                status: 0,
                wall_ns: 20,
                peak_rss_bytes: Some(200),
                stdout: String::new(),
                stderr: String::new(),
            },
        ];
        assert_eq!(summary(&samples), (Some(20), Some(200)));
    }
}
