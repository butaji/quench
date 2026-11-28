//! A CommonJS entry run by `quench x.cjs`: the loader calls its wrapper once, so the facts solve the wrapper's body as code
//! that runs once and its top-level closures keep their identity, from a fresh compile and from the compile cache alike.
//! Legacy reflection never hands out the wrapper (`f.caller` is `null` at the top of the module).
use std::process::Command;

/// Runs the fixture with the AOT facts trace and the cache directory `cache`: its stdout and the trace's closures-once line.
fn run(cache: &std::path::Path) -> (String, String) {
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/commonjs_once.cjs");
    let output = Command::new(env!("CARGO_BIN_EXE_quench"))
        .arg("--no-jit")
        .arg(&fixture)
        .env("QUENCH_TRACE", "aot-lanes")
        .env("XDG_CACHE_HOME", cache)
        .env("QUENCH_STARTUP_SNAPSHOT", "off")
        .env_remove("NODE_OPTIONS")
        .output()
        .expect("run quench");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{stderr}");
    let once = stderr.lines().rev().find(|line| line.contains("closures once") && !line.contains("once body 0")).unwrap_or("").to_owned();
    (String::from_utf8_lossy(&output.stdout).into_owned(), once)
}

#[test]
fn a_commonjs_entry_runs_its_body_once_from_a_compile_and_from_the_cache() {
    let cache = std::env::temp_dir().join(format!("quench-commonjs-once-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    for pass in ["compiled", "cached"] {
        let (stdout, once) = run(&cache);
        assert_eq!(stdout.trim(), "sum 6000000 caller null", "{pass}: the wrapper is no caller");
        assert!(once.contains("once body 1,"), "{pass}: the wrapper's body runs once: {once:?}");
        // The wrapper, `S`, both prototype methods and `f`: every top-level closure is one object.
        assert!(once.contains("{1, 2, 3, 4, 5}"), "{pass}: the top-level closures keep their identity: {once:?}");
    }
    let _ = std::fs::remove_dir_all(&cache);
}
