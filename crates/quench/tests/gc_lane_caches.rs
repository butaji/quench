//! A lane run that continues past a collection a native call made (`gc()`) resolves again
//! what it cached before it: call targets, function targets, read memos and capture storages
//! (TOD-1807). Node prints the same two lines.
use std::process::Command;

#[test]
fn lane_caches_retire_at_a_collection_in_every_mode() {
    let fixture = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/gc_lane_caches.cjs");
    // `--` ends the options: the default mode, every tier on.
    for flag in ["--no-jit", "--"] {
        let output = Command::new(env!("CARGO_BIN_EXE_quench"))
            .arg("--expose-gc")
            .arg(flag)
            .arg(fixture)
            .env_remove("NODE_OPTIONS")
            .output()
            .expect("run quench");
        assert!(output.status.success(), "{flag}: {}", String::from_utf8_lossy(&output.stderr));
        assert_eq!(String::from_utf8_lossy(&output.stdout), "1969471\n1800615560\n", "{flag}");
    }
}
