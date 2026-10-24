//! Measurement-only orchestration of the pinned upstream HTTP framework suite.
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::Write,
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command},
    thread,
    time::{Duration, Instant},
};

const UPSTREAM: &str = "f654bcd175165bce2e3c83b65eaecd8e34294ffd";
const URL: &str = "http://127.0.0.1:3000";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);
const COMMAND_TIMEOUT_SECS: u64 = 180;
const COMMAND_KILL_GRACE_SECS: u64 = 5;
const MAX_LOAD_SECS: u64 = 120;
const DEFAULT_TARGETS: &str = "quench/express,quench/fastify,quench/hono,node/express,node/fastify,node/hono,bun/elysia,bun/hono,bun/fastify";

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn output(program: &str, args: &[&str], cwd: &Path, vars: &[(&str, &str)]) -> Result<String> {
    let result = Command::new("timeout")
        .arg(format!("--kill-after={COMMAND_KILL_GRACE_SECS}s"))
        .arg(format!("{COMMAND_TIMEOUT_SECS}s"))
        .arg(program)
        .args(args)
        .current_dir(cwd)
        .env_remove("NODE_OPTIONS")
        .env_remove("BUN_OPTIONS")
        .envs(vars.iter().copied())
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    if !result.status.success() {
        return Err(format!("{program} {args:?}: {text}").into());
    }
    Ok(text)
}
fn hash(path: &Path) -> Result<String> {
    Ok(output(
        "sha256sum",
        &[path.to_str().ok_or("non-UTF8 path")?],
        Path::new("."),
        &[],
    )?
    .split_whitespace()
    .next()
    .ok_or("missing hash")?
    .into())
}
fn executable(name: &str) -> Result<PathBuf> {
    if name.contains('/') {
        return Ok(fs::canonicalize(name)?);
    }
    env::split_paths(&env::var_os("PATH").ok_or("PATH missing")?)
        .map(|p| p.join(name))
        .find(|p| p.is_file())
        .ok_or_else(|| format!("executable missing: {name}").into())
}
fn curl(suite: &Path, path: &str, post: bool, headers: &Path, body: &Path) -> Result<()> {
    let url = format!("{URL}{path}");
    let mut args = vec![
        "--silent",
        "--show-error",
        "--noproxy",
        "*",
        "--max-time",
        "15",
        "-D",
        headers.to_str().unwrap(),
        "-o",
        body.to_str().unwrap(),
        "-w",
        "%{http_code}",
        "-H",
        "Cache-Control:no-store",
        "-H",
        "If-None-Match:\"benchmark-force-full-response\"",
    ];
    if post {
        args.extend([
            "-H",
            "Content-Type:application/json",
            "--data-binary",
            "@scripts/body.json",
        ]);
    }
    args.push(&url);
    if output("curl", &args, suite, &[])?.trim() != "200" {
        return Err(format!("non-200 response: {path}").into());
    }
    Ok(())
}
fn validate(suite: &Path, dir: &Path) -> BTreeMap<String, String> {
    let mut checks = BTreeMap::new();
    for (name, path, expected) in [
        ("ping", "/", "Hi"),
        ("query", "/id/1?name=bun", "1 bun"),
        ("query-dynamic", "/id/42?other=x&name=alice", "42 alice"),
        ("body", "/json", ""),
        ("video", "/video", ""),
    ] {
        let check = (|| -> Result<()> {
            let headers = dir.join(format!("{name}.headers"));
            let body = dir.join(format!("{name}.body"));
            curl(suite, path, name == "body", &headers, &body)?;
            let h = fs::read_to_string(headers)?.to_ascii_lowercase();
            let content_type = match name {
                "body" => "application/json",
                "video" => "video/mp4",
                _ => "text/plain",
            };
            if !h.contains(&format!("content-type: {content_type}")) {
                return Err(format!("wrong content type: {name}").into());
            }
            if name.starts_with("query") && !h.contains("x-powered-by: benchmark") {
                return Err("missing query header".into());
            }
            match name {
                "body" => {
                    let actual: Value = serde_json::from_slice(&fs::read(&body)?)?;
                    let expected: Value =
                        serde_json::from_slice(&fs::read(suite.join("scripts/body.json"))?)?;
                    if actual != expected {
                        return Err("JSON round-trip mismatch".into());
                    }
                }
                "video" => {
                    let source = suite.join("public/kyuukurarin.mp4");
                    let actual = hash(&body)?;
                    let expected = hash(&source)?;
                    let size = fs::metadata(&body)?.len();
                    fs::remove_file(body)?;
                    if actual != expected {
                        return Err(format!("streamed video mismatch: received {size} bytes, hash {actual}, expected {expected}").into());
                    }
                }
                _ => {
                    if fs::read_to_string(body)? != expected {
                        return Err(format!("body mismatch: {name}").into());
                    }
                }
            }
            Ok(())
        })();
        checks.insert(
            name.into(),
            match check {
                Ok(()) => "passed".into(),
                Err(e) => e.to_string(),
            },
        );
    }
    checks
}
fn validate_load(result: &Value) -> Result<()> {
    let stats = &result["result"];
    let rps = stats["rps"]["mean"].as_f64().ok_or("missing throughput")?;
    if !rps.is_finite()
        || rps <= 0.
        || stats["req2xx"].as_u64().ok_or("missing success count")? == 0
    {
        return Err("no successful requests".into());
    }
    if let Some(errors) = stats.get("errors") {
        let empty = match errors {
            Value::Null => true,
            Value::Number(n) => n.as_u64() == Some(0),
            Value::Array(a) => a.is_empty(),
            Value::Object(o) => o.is_empty(),
            _ => false,
        };
        if !empty {
            return Err("load generator reported errors".into());
        }
    }
    for key in ["req1xx", "req3xx", "req4xx", "req5xx", "others"] {
        if stats[key].as_u64().ok_or("missing status count")? != 0 {
            return Err(format!("non-successful request count: {key}").into());
        }
    }
    Ok(())
}

fn sample(
    suite: &Path,
    dir: &Path,
    binary: &str,
    runtime: &str,
    entry: &Path,
    load: &str,
    seconds: u64,
) -> Result<Value> {
    if TcpStream::connect("127.0.0.1:3000").is_ok() {
        return Err("port 3000 already occupied; refusing to kill another process".into());
    }
    fs::create_dir(dir)?;
    let mut cmd = Command::new(binary);
    if runtime == "node" {
        cmd.arg("--jitless");
    }
    if runtime == "bun" {
        cmd.env("BUN_JSC_useJIT", "0");
    }
    let start = Instant::now();
    let mut server = Server(
        cmd.arg(entry)
            .current_dir(suite)
            .env_remove("NODE_OPTIONS")
            .env_remove("BUN_OPTIONS")
            .stdout(File::create(dir.join("server.stdout"))?)
            .stderr(File::create(dir.join("server.stderr"))?)
            .spawn()?,
    );
    while TcpStream::connect("127.0.0.1:3000").is_err() {
        if let Some(status) = server.0.try_wait()? {
            return Err(format!("server exited {status}; see server.stderr").into());
        }
        if start.elapsed() > STARTUP_TIMEOUT {
            return Err("server startup timed out".into());
        }
        thread::sleep(POLL);
    }
    let startup_ms = start.elapsed().as_secs_f64() * 1000.;
    let memory_before = fs::read_to_string(format!("/proc/{}/status", server.0.id())).ok();
    let checks = validate(suite, dir);
    let mut samples = Vec::new();
    for (name, path, connections) in [
        ("ping", "/", "500"),
        ("query", "/id/1?name=bun", "500"),
        ("body", "/json", "500"),
        ("video", "/video", "10"),
    ] {
        let error = checks.iter().find(|(key, value)| {
            (key.as_str() == name || (name == "query" && key.as_str() == "query-dynamic"))
                && value.as_str() != "passed"
        });
        if let Some((_, error)) = error {
            samples.push(json!({"workload":name,"valid":false,"error":error}));
            continue;
        }
        if seconds == 0 {
            samples.push(json!({"workload":name,"valid":true,"verification_only":true}));
            continue;
        }
        let url = format!("{URL}{path}");
        let duration = format!("{seconds}s");
        let mut args = vec![
            "-o",
            "json",
            "-p",
            "r",
            "--latencies",
            "--fasthttp",
            "-c",
            connections,
            "-d",
            &duration,
        ];
        if name == "body" {
            args.extend([
                "-m",
                "POST",
                "-H",
                "Content-Type:application/json",
                "-f",
                "scripts/body.json",
            ]);
        }
        if name == "video" {
            args.extend([
                "-H",
                "Cache-Control:no-store",
                "-H",
                "If-None-Match:\"benchmark-force-full-response\"",
            ]);
        }
        args.push(&url);
        let raw = output(load, &args, suite, &[])?;
        fs::write(dir.join(format!("{name}.json")), &raw)?;
        let result: Value = serde_json::from_str(&raw)?;
        let validation = validate_load(&result);
        samples.push(json!({"workload":name,"valid":validation.is_ok(),"error":validation.err().map(|e|e.to_string()),"raw":result}));
    }
    let memory_after = fs::read_to_string(format!("/proc/{}/status", server.0.id())).ok();
    let valid = samples.iter().all(|s| s["valid"] == true);
    Ok(
        json!({"valid":valid,"startup_ms":startup_ms,"correctness":checks,"workloads":samples,"linux_proc_status_before":memory_before,"linux_proc_status_after":memory_after}),
    )
}
fn run() -> Result<()> {
    let mut options = BTreeMap::new();
    let mut args = env::args().skip(1);
    while let Some(key) = args.next() {
        if key == "--help" {
            println!(
                "web --suite PATH --out NEW_DIRECTORY --quench PATH [--node node] [--bun bun] [--bombardier bombardier] [--targets {DEFAULT_TARGETS}] [--seconds 10] [--rounds 3] [--verify-only]"
            );
            return Ok(());
        }
        if key == "--verify-only" {
            options.insert(key, "true".into());
            continue;
        }
        if ![
            "--suite",
            "--out",
            "--quench",
            "--node",
            "--bun",
            "--bombardier",
            "--targets",
            "--seconds",
            "--rounds",
        ]
        .contains(&key.as_str())
        {
            return Err(format!("unknown option {key}").into());
        }
        options.insert(key, args.next().ok_or("missing option value")?);
    }
    let required = |k: &str| -> Result<&str> {
        options
            .get(k)
            .map(String::as_str)
            .ok_or_else(|| format!("missing {k}").into())
    };
    let suite = fs::canonicalize(required("--suite")?)?;
    if output("git", &["rev-parse", "HEAD"], &suite, &[])?.trim() != UPSTREAM {
        return Err("wrong upstream revision".into());
    }
    if !output(
        "git",
        &["status", "--porcelain", "--untracked-files=no"],
        &suite,
        &[],
    )?
    .trim()
    .is_empty()
    {
        return Err("upstream tracked files modified".into());
    }
    let out = PathBuf::from(required("--out")?);
    fs::create_dir(&out)?;
    let out = fs::canonicalize(out)?;
    let mut seconds: u64 = options
        .get("--seconds")
        .map(String::as_str)
        .unwrap_or("10")
        .parse()?;
    let rounds: usize = options
        .get("--rounds")
        .map(String::as_str)
        .unwrap_or("3")
        .parse()?;
    if seconds == 0 || seconds > MAX_LOAD_SECS || rounds == 0 {
        return Err(
            "seconds must be 1..120 (command timeout is 180s), rounds must be positive".into(),
        );
    }
    if options.contains_key("--verify-only") {
        seconds = 0;
    }
    let targets: Vec<_> = options
        .get("--targets")
        .map(String::as_str)
        .unwrap_or(DEFAULT_TARGETS)
        .split(',')
        .collect();
    let mut engines = BTreeMap::new();
    for runtime in ["quench", "node", "bun"] {
        if !targets
            .iter()
            .any(|t| t.starts_with(&format!("{runtime}/")))
        {
            continue;
        }
        let key = format!("--{runtime}");
        let name = if runtime == "quench" {
            required(&key)?
        } else {
            options.get(&key).map(String::as_str).unwrap_or(runtime)
        };
        let identity = (|| -> Result<Value> {
            let binary = executable(name)?;
            let bin = binary.to_str().ok_or("binary path not UTF8")?;
            let version = output(bin, &["--version"], &suite, &[])?;
            let proof = match runtime {
                "node" => output(
                    bin,
                    &[
                        "--jitless",
                        "-p",
                        "console.log(process.execArgv.includes('--jitless'))",
                    ],
                    &suite,
                    &[],
                )?,
                "bun" => {
                    let proof = output(
                        bin,
                        &["-e", "0"],
                        &suite,
                        &[("BUN_JSC_useJIT", "0"), ("BUN_JSC_dumpOptions", "1")],
                    )?;
                    if !proof.contains("useJIT=false") {
                        return Err("Bun no-JIT proof missing".into());
                    }
                    proof
                }
                _ => {
                    "quench-node uses the default interpreted execution mode; no compiled-mode flag"
                        .into()
                }
            };
            Ok(
                json!({"binary":binary,"sha256":hash(&binary)?,"version":version,"jit_proof":proof,"server_args":if runtime == "node" {vec!["--jitless"]} else {vec![]},"server_env":if runtime == "bun" {json!({"BUN_JSC_useJIT":"0"})} else {json!({})}}),
            )
        })();
        engines.insert(
            runtime,
            match identity {
                Ok(v) => v,
                Err(e) => json!({"error":e.to_string()}),
            },
        );
    }
    let load = executable(
        options
            .get("--bombardier")
            .map(String::as_str)
            .unwrap_or("bombardier"),
    )?;
    let runner = env::current_exe()?;
    let mut report = json!({"runner":{"binary":runner,"sha256":hash(&runner)?},"verification_only":seconds==0,"upstream":UPSTREAM,"source":output("git", &["rev-parse","HEAD"], Path::new("."), &[])?,"source_status":output("git", &["status","--porcelain","--untracked-files=no"], Path::new("."), &[])?,"host":output("uname", &["-a"],Path::new("."),&[])?,"toolchain":output("rustc", &["--version"],Path::new("."),&[])?,"upstream_lock_sha256":hash(&suite.join("bun.lockb"))?,"cpu_limits":fs::read_to_string("/sys/fs/cgroup/cpu.max").ok(),"memory_limit":fs::read_to_string("/sys/fs/cgroup/memory.max").ok(),"load_generator":{"binary":load,"sha256":hash(&load)?,"version":output(load.to_str().unwrap(), &["--version"], &suite, &[])?},"seconds":seconds,"rounds":rounds,"engines":engines,"samples":[],"complete":false});
    let mut complete = true;
    for round in 0..rounds {
        for offset in 0..targets.len() {
            let target = targets[(round + offset) % targets.len()];
            let attempt = (|| -> Result<Value> {
                let (runtime, framework) = target
                    .split_once('/')
                    .ok_or("target must be runtime/framework")?;
                if !["node", "bun", "quench"].contains(&runtime)
                    || framework.contains('/')
                    || framework.contains("..")
                {
                    return Err("unsupported target".into());
                }
                let identity = engines.get(runtime).ok_or("runtime not preflighted")?;
                let binary = identity["binary"]
                    .as_str()
                    .ok_or("runtime unavailable; see engines")?;
                let source_runtime = if runtime == "quench" { "node" } else { runtime };
                let build_target = format!("{source_runtime}/{framework}");
                let bun = options.get("--bun").map(String::as_str).unwrap_or("bun");
                output(
                    bun,
                    &["scripts/build-framework.ts", &build_target],
                    &suite,
                    &[],
                )?;
                let entry = suite.join(format!("dist/{build_target}/index.js"));
                let dir = out.join(format!("round-{round}-{}", target.replace('/', "-")));
                let result = sample(
                    &suite,
                    &dir,
                    binary,
                    runtime,
                    &entry,
                    load.to_str().unwrap(),
                    seconds,
                )?;
                Ok(json!({"result":result,"entry_sha256":hash(&entry)?}))
            })();
            let row = match attempt {
                Ok(v) => {
                    let valid = v["result"]["valid"] == true;
                    complete &= valid;
                    json!({"target":target,"round":round,"valid":valid,"sample":v})
                }
                Err(e) => {
                    complete = false;
                    json!({"target":target,"round":round,"valid":false,"error":e.to_string()})
                }
            };
            println!("{target} round {round}: {}", row["valid"]);
            report["samples"].as_array_mut().unwrap().push(row);
            fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
        }
    }
    report["complete"] = json!(complete);
    let mut final_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(out.join("final.json"))?;
    final_file.write_all(&serde_json::to_vec_pretty(&report)?)?;
    if !complete {
        return Err("incomplete benchmark: see retained report".into());
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("web: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_failed_or_incomplete_load_reports() {
        let good = json!({"result":{"req1xx":0,"req2xx":10,"req3xx":0,"req4xx":0,"req5xx":0,"others":0,"rps":{"mean":10.0}}});
        assert!(validate_load(&good).is_ok());
        for invalid in [json!({}), json!({"result":{"rps":{"mean":10.0}}})] {
            assert!(validate_load(&invalid).is_err());
        }
        for (key, value) in [
            ("req2xx", json!(0)),
            ("req5xx", json!(1)),
            ("errors", json!({"timeout":1})),
        ] {
            let mut bad = good.clone();
            bad["result"][key] = value;
            assert!(validate_load(&bad).is_err());
        }
    }
}
