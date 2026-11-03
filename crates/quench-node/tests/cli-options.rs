use std::process::Command;

const SOURCE: &str = include_str!("fixtures/internal-options.cjs");

#[test]
fn internal_options_matches_node_in_interpreter_and_jit() {
    for arguments in [
        vec![],
        vec![
            "--no-warnings",
            "--conditions=one",
            "--conditions=two",
            "--report-directory=chosen",
            "--report-compact",
            "--test-random-seed=7",
        ],
    ] {
        let expected = Command::new("node")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .args(["--expose-internals"])
            .args(&arguments)
            .args(["-e", SOURCE])
            .output()
            .unwrap();
        assert!(
            expected.status.success(),
            "Node {}: {}",
            expected.status,
            String::from_utf8_lossy(&expected.stderr)
        );
        for mode in [None, Some("--jit")] {
            let actual = Command::new(env!("CARGO_BIN_EXE_quench-node"))
                .env_clear()
                .args(mode)
                .arg("--expose-internals")
                .args(&arguments)
                .args(["-e", SOURCE])
                .output()
                .unwrap();
            assert!(
                actual.status.success(),
                "{mode:?}: {}",
                String::from_utf8_lossy(&actual.stderr)
            );
            let actual_value: serde_json::Value = serde_json::from_slice(&actual.stdout).unwrap();
            let expected_value: serde_json::Value =
                serde_json::from_slice(&expected.stdout).unwrap();
            for (key, value) in expected_value.as_object().unwrap() {
                assert_eq!(&actual_value[key], value, "{mode:?} {arguments:?} {key}");
            }
            assert_eq!(actual.stderr, expected.stderr);
        }
    }
}

#[test]
fn internal_options_visibility_matches_node() {
    let source = "for (const name of ['internal/options', 'node:internal/options']) { try { require(name); console.log('loaded'); } catch (e) { console.log(e.code); } }";
    for arguments in [vec![], vec!["--expose-internals"]] {
        let expected = Command::new("node")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .args(&arguments)
            .args(["-e", source])
            .output()
            .unwrap();
        let actual = Command::new(env!("CARGO_BIN_EXE_quench-node"))
            .env_clear()
            .args(&arguments)
            .args(["-e", source])
            .output()
            .unwrap();
        assert!(actual.status.success());
        assert_eq!(actual.stdout, expected.stdout);
        assert_eq!(actual.stderr, expected.stderr);
    }
}

#[test]
fn internal_options_esm_exports_match_node() {
    let source = "import options, * as exports from 'internal/options'; import other from 'node:internal/options'; console.log(JSON.stringify([options === other, Object.keys(exports), exports.getOptionValue === undefined, options.getOptionValue('--warnings')]));";
    let expected = Command::new("node")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .args(["--expose-internals", "--input-type=module", "-e", source])
        .output()
        .unwrap();
    let actual = Command::new(env!("CARGO_BIN_EXE_quench-node"))
        .env_clear()
        .args([
            "--jit",
            "--expose-internals",
            "--input-type=module",
            "-e",
            source,
        ])
        .output()
        .unwrap();
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    assert!(
        actual.status.success(),
        "{}",
        String::from_utf8_lossy(&actual.stderr)
    );
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(actual.stderr, expected.stderr);
}

#[cfg(feature = "aot")]
#[test]
#[ignore = "explicit execution-only production SDK qualification"]
fn internal_options_match_node_without_source_in_native_js_and_ts() {
    use quench_node::aot::{NativeSupport, SourceManifest};
    use std::{fs, path::Path};
    let directory =
        std::env::temp_dir().join(format!("quench-internal-options-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let support = NativeSupport::from_cargo_report(
        &fs::read_to_string(
            std::env::var_os("QUENCH_INTERNAL_OPTIONS_SDK_REPORT")
                .expect("production execution-only Cargo report"),
        )
        .unwrap(),
    )
    .unwrap();
    let arguments = [
        "--expose-internals",
        "--no-warnings",
        "--conditions=one",
        "--conditions=two",
        "--report-directory=chosen",
        "--report-compact",
        "--test-random-seed=7",
    ];
    // Exposing private modules is an explicit embedder setting, not an argv
    // convention added to the production native launcher.
    let driver =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/aot/image_driver.rs"))
            .unwrap()
            .replace(
                ".with_exec_argv(Vec::new())?",
                &format!(
                    ".with_exec_argv(vec![{}])?",
                    arguments
                        .iter()
                        .map(|argument| format!("{argument:?}.into()"))
                        .collect::<Vec<_>>()
                        .join(",")
                ),
            );
    let driver_path = directory.join("driver.rs");
    fs::write(&driver_path, driver).unwrap();
    for extension in ["cjs", "cts"] {
        let source = directory.join(format!("entry.{extension}"));
        fs::write(
            &source,
            if extension == "cts" {
                format!("const typed: number = 1;\n{SOURCE}")
            } else {
                SOURCE.to_owned()
            },
        )
        .unwrap();
        let expected = Command::new("node")
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .args(arguments)
            .arg(&source)
            .output()
            .unwrap();
        assert!(
            expected.status.success(),
            "{}",
            String::from_utf8_lossy(&expected.stderr)
        );
        let mut manifest = SourceManifest::default();
        let graph = manifest.add_module_graph(&source).unwrap();
        manifest
            .extend(SourceManifest::capture_for_graph(&graph).unwrap())
            .unwrap();
        let inputs = manifest
            .compile()
            .unwrap()
            .emit(&directory.join(extension))
            .unwrap();
        let binary = directory.join(format!("native-{extension}"));
        support
            .link_audited(&inputs, &driver_path, &binary)
            .unwrap();
        fs::remove_file(source).unwrap();
        let actual = Command::new(&binary).env_clear().output().unwrap();
        assert_eq!(
            actual.status.code(),
            expected.status.code(),
            "{}",
            String::from_utf8_lossy(&actual.stderr)
        );
        assert_eq!(actual.stderr, expected.stderr);
        assert_eq!(actual.stdout, expected.stdout, "native {extension}");
    }
    fs::remove_dir_all(directory).unwrap();
}
