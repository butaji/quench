use super::*;

#[test]
fn isolated_message_extracts_reason_line_from_stdout() {
    let stdout = b"header\nFAILED\nReason: TypeError: cannot read prop\ntrailer\n";
    let stderr = b"";
    assert_eq!(
        isolated_message(stderr, stdout),
        "TypeError: cannot read prop"
    );
}

#[test]
fn isolated_message_searches_stdout_before_stderr_for_reason() {
    let stdout = b"Reason: stdout-reason\n";
    let stderr = b"header\nReason: stderr-reason\n";
    assert_eq!(isolated_message(stderr, stdout), "stdout-reason");
}

#[test]
fn isolated_message_falls_back_to_failed_line() {
    let stdout = b"header\n\xE2\x9D\x8C FAILED\nfollow-up line without reason\n";
    let stderr = b"";
    assert_eq!(isolated_message(stderr, stdout), "\u{274C} FAILED");
}

#[test]
fn isolated_message_falls_back_to_first_nonempty_stderr_line() {
    let stdout = b"";
    let stderr = b"\n\nfirst real stderr line\nsecond\n";
    assert_eq!(isolated_message(stderr, stdout), "first real stderr line");
}

#[test]
fn isolated_message_returns_last_stdout_line_when_no_known_markers() {
    let stdout = b"alpha\nbeta\ngamma\n";
    let stderr = b"";
    assert_eq!(isolated_message(stderr, stdout), "gamma");
}

#[test]
fn isolated_message_handles_empty_streams() {
    assert_eq!(isolated_message(b"", b""), "");
}

#[test]
fn isolated_message_handles_multiline_diagnostic() {
    let stdout = b"=== TEST ===\nFAILED\nReason: Test262Error: multi\n  line\n  diagnostic\n";
    let stderr = b"";
    assert_eq!(isolated_message(stderr, stdout), "Test262Error: multi");
}

#[test]
fn isolated_message_ignores_failed_marker_without_reason_in_stdout() {
    let stdout = b"FAILED only marker\nno reason on next line\n";
    let stderr = b"";
    assert_eq!(isolated_message(stderr, stdout), "FAILED only marker");
}

fn real_exit_status(code: i32) -> std::process::ExitStatus {
    use std::process::Command;
    if code == 0 {
        Command::new("true").status().expect("true exits 0")
    } else {
        Command::new("sh")
            .arg("-c")
            .arg(format!("exit {code}"))
            .status()
            .expect("sh exit N")
    }
}

fn signal_terminated_status() -> std::process::ExitStatus {
    use std::process::Command;
    Command::new("sh")
        .arg("-c")
        .arg("kill -9 $$")
        .status()
        .expect("signal-terminated child")
}

fn output_for(
    status: std::process::ExitStatus,
    stdout: &[u8],
    stderr: &[u8],
) -> std::process::Output {
    std::process::Output {
        status,
        stdout: stdout.to_vec(),
        stderr: stderr.to_vec(),
    }
}

fn fake_isolated_path() -> std::path::PathBuf {
    std::path::PathBuf::from(crate::runner::default_test262_dir())
        .join("test/harness/assert-obj.js")
}

#[test]
fn classification_classify_isolated_table_driven_exit_paths() {
    let path = fake_isolated_path();
    let cases: Vec<(&str, std::process::ExitStatus, Vec<u8>, Vec<u8>)> = vec![
        (
            "exit-zero-with-empty-output-passes",
            real_exit_status(0),
            Vec::new(),
            Vec::new(),
        ),
        (
            "exit-one-empty-output-fails-with-explicit-diagnostic",
            real_exit_status(1),
            Vec::new(),
            Vec::new(),
        ),
        (
            "exit-one-failed-marker-only-fails-with-explicit-diagnostic",
            real_exit_status(1),
            b"\xE2\x9D\x8C FAILED\n".to_vec(),
            Vec::new(),
        ),
        (
            "exit-one-reason-line-fails-with-structured-reason",
            real_exit_status(1),
            b"FAILED\nReason: Test262Error: deep equal mismatch\n".to_vec(),
            Vec::new(),
        ),
        (
            "exit-one-multiline-reason-context-fails-with-structured-diagnostic",
            real_exit_status(1),
            b"FAILED\nReason: Test262Error: multi\n  line-1\n  line-2\n  line-3\n".to_vec(),
            Vec::new(),
        ),
        (
            "exit-one-type-and-message-fields-populate-structured-failure",
            real_exit_status(1),
            b"FAILED\nReason: TypeError: boom\nType: TypeError\nJS message: boom\n".to_vec(),
            Vec::new(),
        ),
        (
            "exit-code-none-signal-killed-is-explicit-diagnostic-not-pass",
            signal_terminated_status(),
            Vec::new(),
            Vec::new(),
        ),
    ];
    for (label, status, stdout, stderr) in cases {
        let out = output_for(status, &stdout, &stderr);
        let outcome = classify_isolated(&out, &path);
        match label {
            "exit-zero-with-empty-output-passes" => {
                assert!(matches!(outcome, TestOutcome::Pass), "{label}: {outcome:?}");
            }
            "exit-one-type-and-message-fields-populate-structured-failure" => {
                let TestOutcome::Fail { failure } = outcome else {
                    panic!("{label}: expected Fail, got {outcome:?}");
                };
                assert!(
                    failure.message.contains("TypeError: boom"),
                    "{label}: {failure:?}"
                );
                assert_eq!(
                    failure.error_type.as_deref(),
                    Some("TypeError"),
                    "{label}: {failure:?}"
                );
                assert_eq!(
                    failure.error_message.as_deref(),
                    Some("boom"),
                    "{label}: {failure:?}"
                );
                assert_eq!(
                    failure.source_path.as_deref(),
                    Some(path.to_string_lossy().as_ref()),
                    "{label}"
                );
            }
            "exit-code-none-signal-killed-is-explicit-diagnostic-not-pass" => {
                let TestOutcome::Fail { failure } = outcome else {
                    panic!("{label}: expected Fail, got {outcome:?}");
                };
                assert!(
                    failure.message.starts_with("isolated terminated by signal"),
                    "{label}: {failure:?}"
                );
                assert_eq!(
                    failure.source_path.as_deref(),
                    Some(path.to_string_lossy().as_ref()),
                    "{label}"
                );
            }
            _ => {
                let TestOutcome::Fail { failure } = outcome else {
                    panic!("{label}: expected Fail, got {outcome:?}");
                };
                assert!(
                    failure.message.contains("isolated exit"),
                    "{label}: {failure:?}"
                );
                assert_eq!(
                    failure.source_path.as_deref(),
                    Some(path.to_string_lossy().as_ref()),
                    "{label}"
                );
            }
        }
    }
}

#[test]
fn classification_classify_isolated_preserves_path_on_malformed_output() {
    let path = fake_isolated_path();
    let out = output_for(real_exit_status(2), b"no markers here, just noise\n", b"");
    let outcome = classify_isolated(&out, &path);
    let TestOutcome::Fail { failure } = outcome else {
        panic!("expected Fail, got Pass for malformed-output + nonzero exit");
    };
    assert_eq!(
        failure.source_path.as_deref(),
        Some(path.to_string_lossy().as_ref())
    );
    assert!(failure.message.contains("isolated exit 2"));
}

#[test]
fn classification_classify_isolated_multiline_reason_keeps_source_context() {
    let path = fake_isolated_path();
    let stdout = b"=== TEST ===\nFAILED\nReason: Test262Error: stepped multi-line\n  fragment one\n  fragment two\n";
    let out = output_for(real_exit_status(1), stdout, b"");
    let outcome = classify_isolated(&out, &path);
    let TestOutcome::Fail { failure } = outcome else {
        panic!("expected Fail");
    };
    assert!(
        failure.message.contains("Test262Error"),
        "fail message carries the structured reason: {failure:?}"
    );
    assert!(
        !failure.source_context.is_empty(),
        "source context is populated from the real test path: {failure:?}"
    );
    assert_eq!(
        failure.source_path.as_deref(),
        Some(path.to_string_lossy().as_ref())
    );
}
