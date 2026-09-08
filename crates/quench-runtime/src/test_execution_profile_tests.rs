use super::*;

fn execute_profile(
    case: &ExecutionCase,
) -> Result<(crate::value::Value, ExecutionProfile), String> {
    let policy = crate::stencil_policy::ExecutionPolicy::arm_opt_in_for_test();
    crate::stencil_policy::with_policy_for_test(policy, || execute_with_policy(case))
}

fn execute_with_policy(
    case: &ExecutionCase,
) -> Result<(crate::value::Value, ExecutionProfile), String> {
    let program = crate::reduce::reduce_source(case.source())
        .map_err(|errors| format!("lowering failed: {}", errors.join("; ")))?;
    let context = crate::vm::current_context_or_default();
    for _ in 0..case.warmup() {
        execute_once(program.code(), &context, false)
            .map_err(|error| format!("warmup failed: {error:?}"))?;
    }
    execute_once(program.code(), &context, true)
        .map_err(|error| format!("profiled execution failed: {error:?}"))
}

fn execute_once(
    code: crate::machine::CodeView<'_>,
    context: &crate::vm::VmContext,
    measured: bool,
) -> Result<(crate::value::Value, ExecutionProfile), crate::execute::VmError> {
    let initialized = crate::vm::execute_code_with_context(code, context)?;
    let Some(prepared) = prepare_execution(&initialized)? else {
        return capture_result(measured, || Ok(initialized));
    };
    let (result, profile) = capture_result(measured, || {
        invoke(context, &prepared.run, &prepared.arguments)
    })?;
    let mut profile = profile;
    profile.lowered_route = lowered_route(&prepared.run);
    let verified = invoke(context, &prepared.verify, &[result])?;
    Ok((verified, profile))
}

fn lowered_route(function: &crate::value::Value) -> Vec<&'static str> {
    let crate::value::Value::Function(function) = function else {
        return Vec::new();
    };
    let Some(code) = function.code.code() else {
        return Vec::new();
    };
    (0..code.len())
        .filter_map(|pc| {
            code.instruction(pc)
                .map(|instruction| instruction.opcode.name())
        })
        .collect()
}

fn capture_result<T>(
    measured: bool,
    execute: impl FnOnce() -> Result<T, crate::execute::VmError>,
) -> Result<(T, ExecutionProfile), crate::execute::VmError> {
    if measured {
        let (result, profile) = capture(execute);
        return result.map(|result| (result, profile));
    }
    execute().map(|result| (result, ExecutionProfile::default()))
}

fn case_mismatch(name: &str) -> Option<String> {
    let case = ExecutionCase::load(name);
    let (result, profile) = match execute_profile(&case) {
        Ok(execution) => execution,
        Err(error) => return Some(format!("{name}: {error}")),
    };
    let mut differences = case.profile.differences(&profile);
    differences.extend(case.plan.differences(&profile));
    if !case.result.matches(&result) {
        differences.push(format!("result: actual {result:?}"));
    }
    (!differences.is_empty()).then(|| format!("{name}: {}", differences.join("; ")))
}

fn isolated_mismatch(name: &str) -> Option<String> {
    let executable = std::env::current_exe().expect("current Rust test executable");
    let output = std::process::Command::new(executable)
        .args([
            "--exact",
            "test_execution_profile::tests::every_json_contract_matches_ideal_execution_profile",
            "--nocapture",
        ])
        .env(PROFILE_CASE_FILTER, name)
        .env(PROFILE_CHILD_PROCESS, "1")
        .output()
        .expect("isolated execution-profile process");
    child_mismatch(name, &output)
}

fn child_mismatch(name: &str, output: &std::process::Output) -> Option<String> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(line) = stdout.lines().find_map(profile_mismatch_line) {
        return Some(line.to_owned());
    }
    (!output.status.success()).then(|| child_failure(name, output))
}

fn profile_mismatch_line(line: &str) -> Option<&str> {
    line.strip_prefix(PROFILE_MISMATCH_MARKER)
}

fn child_failure(name: &str, output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!(
        "{name}: isolated process {:?}\n{stdout}{stderr}",
        output.status
    )
}

#[test]
fn capture_is_invocation_local_and_deterministic() {
    let (_, first) = capture(|| {
        residual("Add");
        stencil("add", true);
    });
    let (_, second) = capture(|| residual("Return"));
    assert_eq!(first.residual_ops.get("Add"), Some(&1));
    assert_eq!(first.stencils.get("add").unwrap().entries, 1);
    assert_eq!(second.residual_ops.get("Return"), Some(&1));
    assert!(!second.stencils.contains_key("add"));
}

#[test]
fn every_json_contract_has_a_complete_standalone_js_case() {
    let names = fixture_names();
    assert!(!names.is_empty(), "execution-profile cases must exist");
    for name in names {
        let case = ExecutionCase::load(&name);
        assert!(!case.source().trim().is_empty(), "empty JS case: {name}");
        crate::reduce::reduce_source(case.source())
            .unwrap_or_else(|errors| panic!("{name} does not lower: {}", errors.join("; ")));
    }
}

#[test]
fn every_json_contract_matches_ideal_execution_profile() {
    if std::env::var_os(PROFILE_CHILD_PROCESS).is_some() {
        emit_selected_mismatch();
        return;
    }
    let names = fixture_names();
    let isolated = std::env::var_os(PROFILE_CASE_FILTER).is_none();
    let mismatches = names
        .iter()
        .filter_map(|name| {
            if isolated {
                isolated_mismatch(name)
            } else {
                case_mismatch(name)
            }
        })
        .collect::<Vec<_>>();
    assert!(
        mismatches.is_empty(),
        "{} execution-profile mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

fn emit_selected_mismatch() {
    let name = std::env::var(PROFILE_CASE_FILTER).expect("selected profile case");
    if let Some(mismatch) = case_mismatch(&name) {
        println!("{PROFILE_MISMATCH_MARKER}{mismatch}");
    }
}
