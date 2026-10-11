use std::{process::Command, time::Duration};

#[test]
fn embedding_children_execute_node_arguments_instead_of_the_suite() {
    for (source, exit_code, stdout) in [
        ("console.log('child-entry')", 0, b"child-entry\n".as_slice()),
        ("process.exit(7)", 7, b"".as_slice()),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_run-parallel"));
        command
            .env(quench_node::shared_run::CHILD_RUNNER_ENV, "1")
            .args(["-e", source]);
        let observation =
            quench_node_test::case_process::observe_command(command, Duration::from_secs(5))
                .unwrap();
        assert!(!observation.timed_out);
        assert_eq!(observation.exit_code, Some(exit_code));
        assert_eq!(observation.stdout, stdout);
        assert!(observation.stderr.is_empty());
    }
}
