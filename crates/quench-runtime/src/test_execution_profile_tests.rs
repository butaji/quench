use super::*;

const EXECUTION_PROFILE_CASE_COUNT: usize = 342;

fn execute_case(
    case: &ExecutionCase,
) -> Result<
    (
        crate::value::Value,
        Vec<crate::ir::Opcode>,
        Vec<RawInstruction>,
        Vec<u32>,
    ),
    String,
> {
    execute_case_with_warmup(case, case.warmup())
}

fn execute_case_with_warmup(
    case: &ExecutionCase,
    warmup: u32,
) -> Result<
    (
        crate::value::Value,
        Vec<crate::ir::Opcode>,
        Vec<RawInstruction>,
        Vec<u32>,
    ),
    String,
> {
    let program = crate::reduce::reduce_source(case.source())
        .map_err(|errors| format!("lowering failed: {}", errors.join("; ")))?;
    let context = crate::vm::current_context_or_default();
    for _ in 0..warmup {
        execute_contract(program.code(), &context)
            .map_err(|error| format!("warmup failed: {error:?}"))?;
    }
    let initialized = crate::vm::execute_code_with_context(program.code(), &context)
        .map_err(|error| format!("initialization failed: {error:?}"))?;
    let Some(prepared) = prepare_execution(&initialized)
        .map_err(|error| format!("contract preparation failed: {error:?}"))?
    else {
        return Ok((initialized, Vec::new(), Vec::new(), Vec::new()));
    };
    let ir = hot_ir(&prepared.run);
    let raw = raw_hot_ir(&prepared.run);
    let code_ids = reachable_code_ids(&prepared.run);
    let result = invoke(&context, &prepared.run, &prepared.arguments)
        .map_err(|error| format!("execution failed: {error:?}"))?;
    let verified = invoke(&context, &prepared.verify, &[result])
        .map_err(|error| format!("verification failed: {error:?}"))?;
    Ok((verified, ir, raw, code_ids))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RawInstruction {
    pc: usize,
    opcode: crate::ir::Opcode,
    flags: u8,
    operands: [u16; 3],
    branch_target: Option<u16>,
    cold_variant: Option<&'static str>,
    generic_fallback: Option<&'static str>,
}

/// Capture the physical compact instruction stream before semantic-family
/// normalization. This is intentionally a diagnostic view: JSON expectations
/// remain semantic, while this record makes generic Binary/Slow gateways and
/// operand/control changes visible to the active JIT lowering work.
fn raw_hot_ir(function: &crate::value::Value) -> Vec<RawInstruction> {
    let crate::value::Value::Function(function) = function else {
        return Vec::new();
    };
    let Some(code) = function.code.code() else {
        return Vec::new();
    };
    let reachable = reachable_pcs(code);
    (0..code.len())
        .filter(|pc| reachable[*pc])
        .filter_map(|pc| {
            code.instruction(pc).map(|instruction| {
                let cold = code.cold(instruction);
                RawInstruction {
                    pc,
                    opcode: instruction.opcode,
                    flags: instruction.flags,
                    operands: [instruction.a, instruction.b, instruction.c],
                    cold_variant: cold.map(|op| op.variant_name()),
                    generic_fallback: cold.and_then(|op| op.generic_fallback_name()),
                    branch_target: match instruction.opcode.control_operands(instruction) {
                        crate::ir::ControlOperands::Branch { target, .. }
                        | crate::ir::ControlOperands::Jump { target } => Some(target),
                        _ => None,
                    },
                }
            })
        })
        .collect()
}

/// Capture immutable code-store identities for the run body and all nested
/// structured function bodies. The IDs distinguish shared-store ranges from
/// independent function instances without making nested bytes part of the
/// semantic JSON contract.
fn reachable_code_ids(function: &crate::value::Value) -> Vec<u32> {
    let crate::value::Value::Function(function) = function else {
        return Vec::new();
    };
    let Some(root) = function.code.code() else {
        return Vec::new();
    };
    let mut ids = std::collections::BTreeSet::new();
    collect_code_ids(root, &mut ids);
    ids.into_iter().collect()
}

fn collect_code_ids(code: crate::machine::CodeView<'_>, ids: &mut std::collections::BTreeSet<u32>) {
    ids.insert(code.range().code.0);
    for (_, operation) in code.cold_ops() {
        operation.visit_bodies(&mut |body| {
            if let Some(nested) = body.code() {
                collect_code_ids(nested, ids);
            }
        });
    }
}

/// Return the reachable, post-warmup instruction stream for the run function.
///
/// Unreachable compiler epilogues and out-of-line stencil/fallback bodies are
/// deliberately absent: the fixture describes the hot path that a performance
/// change is expected to improve, while semantic fallback behavior remains
/// covered by the result assertion and focused implementation tests.
fn hot_ir(function: &crate::value::Value) -> Vec<crate::ir::Opcode> {
    let crate::value::Value::Function(function) = function else {
        return Vec::new();
    };
    let Some(code) = function.code.code() else {
        return Vec::new();
    };
    let reachable = reachable_pcs(code);
    (0..code.len())
        .filter(|pc| reachable[*pc])
        .filter_map(|pc| {
            code.instruction(pc)
                .map(|instruction| canonical_hot_opcode(code, instruction))
        })
        .collect()
}

fn canonical_hot_opcode(
    code: crate::machine::CodeView<'_>,
    instruction: crate::ir::Instruction,
) -> crate::ir::Opcode {
    if let Some(operator) = instruction.opcode.binary_operator(instruction.flags) {
        if let Some(opcode) = crate::ir::Opcode::binary_opcode(operator) {
            return opcode;
        }
    }
    if instruction.opcode == crate::ir::Opcode::Slow {
        if let Some(op) = code.cold(instruction) {
            return op.cold_opcode().unwrap_or(crate::ir::Opcode::Slow);
        }
    }
    // Quickening is an execution-view optimization, not a second IR. Keep
    // aliases in the raw physical witness while the JSON contract observes
    // the one canonical semantic opcode spelling.
    instruction.opcode.semantic_opcode()
}

fn reachable_pcs(code: crate::machine::CodeView<'_>) -> Vec<bool> {
    let mut reachable = vec![false; code.len()];
    let mut pending = vec![0usize];
    while let Some(pc) = pending.pop() {
        if pc >= code.len() || reachable[pc] {
            continue;
        }
        reachable[pc] = true;
        let Some(instruction) = code.instruction(pc) else {
            continue;
        };
        match instruction.opcode.control_operands(instruction) {
            crate::ir::ControlOperands::Next | crate::ir::ControlOperands::Loop { .. } => {
                pending.push(pc + 1)
            }
            crate::ir::ControlOperands::Branch { target, .. } => {
                pending.extend([pc + 1, usize::from(target)]);
            }
            crate::ir::ControlOperands::Jump { target } => pending.push(usize::from(target)),
            crate::ir::ControlOperands::Return { .. }
            | crate::ir::ControlOperands::Throw { .. } => {}
        }
    }
    reachable
}

fn case_mismatch(name: &str) -> Option<String> {
    let case = ExecutionCase::load(name);
    let (result, ir, raw, code_ids) = match execute_case(&case) {
        Ok(execution) => execution,
        Err(error) => return Some(format!("{name}: {error}")),
    };
    let mut differences = case.ir_differences(&ir);
    if raw.len() != ir.len() {
        differences.push(format!(
            "physical: raw reachable instruction count {} != semantic view {}",
            raw.len(),
            ir.len()
        ));
    }
    if !differences.is_empty() {
        differences.push(format!("physical: {}", raw_description(&raw)));
    }
    if !case.result.matches(&result) {
        differences.push(format!("result: actual {result:?}"));
    }
    if code_ids.is_empty() {
        differences.push("physical: no reachable code identity".to_owned());
    }
    (!differences.is_empty()).then(|| format!("{name}: {}", differences.join("; ")))
}

fn raw_description(raw: &[RawInstruction]) -> String {
    raw.iter()
        .map(|instruction| {
            format!(
                "pc{}={:?}/f{}/({},{},{}){}",
                instruction.pc,
                instruction.opcode,
                instruction.flags,
                instruction.operands[0],
                instruction.operands[1],
                instruction.operands[2],
                instruction
                    .branch_target
                    .map_or_else(String::new, |target| format!("->{}", target))
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn isolated_mismatch(name: &str) -> Option<String> {
    let executable = std::env::current_exe().expect("current Rust test executable");
    let output = std::process::Command::new(executable)
        .args([
            "--exact",
            "test_execution_profile::tests::every_json_contract_matches_hot_ir",
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
fn raw_physical_ir_preserves_opcode_flags_and_operands() {
    let case = ExecutionCase::load("add_chain");
    let (_, semantic, raw, code_ids) = execute_case(&case).expect("physical execution profile");
    assert_eq!(raw.len(), semantic.len());
    assert!(!raw.is_empty());
    assert!(raw
        .iter()
        .any(|instruction| instruction.opcode == crate::ir::Opcode::Add));
    assert!(raw
        .iter()
        .any(|instruction| instruction.flags != 0 || instruction.operands != [0; 3]));
    assert_eq!(
        code_ids.len(),
        1,
        "flat case has one immutable code identity"
    );
}

#[test]
fn raw_physical_ir_captures_nested_code_identity() {
    let case = ExecutionCase::load("micro_closures_escaping");
    let (_, _, _, code_ids) = execute_case(&case).expect("nested physical execution profile");
    assert!(
        code_ids.len() > 1,
        "nested function body identity must be visible"
    );
    assert!(code_ids.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn warmup_isolation_does_not_mutate_measured_closure_state() {
    let case = ExecutionCase::load("closure_shared_cell");
    for warmup in [0, 1, 3] {
        let (result, _, _, _) = execute_case_with_warmup(&case, warmup)
            .unwrap_or_else(|error| panic!("warmup isolation failed at {warmup}: {error}"));
        case.assert(&result);
    }
}

#[test]
fn every_json_contract_has_a_complete_standalone_js_case() {
    let names = fixture_names();
    assert!(!names.is_empty(), "execution-profile cases must exist");
    if std::env::var_os(PROFILE_CASE_FILTER).is_none() {
        assert_eq!(
            names.len(),
            EXECUTION_PROFILE_CASE_COUNT,
            "the complete execution-profile corpus must run every case"
        );
    }
    for name in names {
        let case = ExecutionCase::load(&name);
        assert!(!case.source().trim().is_empty(), "empty JS case: {name}");
        crate::reduce::reduce_source(case.source())
            .unwrap_or_else(|errors| panic!("{name} does not lower: {}", errors.join("; ")));
    }
}

#[test]
fn every_json_contract_matches_hot_ir() {
    if std::env::var_os(PROFILE_CHILD_PROCESS).is_some() {
        emit_selected_mismatch();
        return;
    }
    let names = fixture_names();
    if std::env::var_os("QUENCH_EXECUTION_PROFILE_PHYSICAL_INVENTORY").is_some() {
        emit_physical_inventory(&names);
        return;
    }
    if std::env::var_os("QUENCH_EXECUTION_PROFILE_ROUTE_INVENTORY").is_some() {
        emit_route_inventory(&names);
        return;
    }
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
        "{} hot-IR mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}

/// The physical witness is deliberately kept outside the JSON schema, but
/// the lowering invariant is still executable: every cold operation observed
/// by the complete corpus must use its declared typed row.  Unclassified ops
/// may continue to use the generic `Slow` fallback until a new row is proven.
#[test]
fn every_profile_cold_operation_uses_declared_opcode_row() {
    if std::env::var_os(PROFILE_CHILD_PROCESS).is_some()
        || std::env::var_os("QUENCH_EXECUTION_PROFILE_PHYSICAL_INVENTORY").is_some()
    {
        return;
    }
    for name in fixture_names() {
        let case = ExecutionCase::load(&name);
        let (_, _, raw, _) = execute_case(&case)
            .unwrap_or_else(|error| panic!("{name} physical lowering failed: {error}"));
        for instruction in raw {
            if instruction.cold_variant.is_some() {
                if instruction.opcode == crate::ir::Opcode::Slow {
                    assert_eq!(
                        instruction.generic_fallback, instruction.cold_variant,
                        "{name} pc {} has an unnamed generic fallback",
                        instruction.pc
                    );
                } else {
                    assert!(
                        instruction.opcode.is_typed_cold_marker(),
                        "{name} pc {} uses generic {:?} for cold {}",
                        instruction.pc,
                        instruction.opcode,
                        instruction.cold_variant.unwrap_or("unknown")
                    );
                    assert_eq!(
                        instruction.generic_fallback, None,
                        "{name} pc {} typed cold row also reported generic fallback",
                        instruction.pc
                    );
                }
            }
        }
    }
}

fn emit_physical_inventory(names: &[String]) {
    let mut inventory = std::collections::BTreeMap::<String, usize>::new();
    for name in names {
        let case = ExecutionCase::load(name);
        let (_, _, raw, _) = execute_case(&case)
            .unwrap_or_else(|error| panic!("{name} physical inventory failed: {error}"));
        for instruction in raw {
            let family = match instruction.opcode {
                crate::ir::Opcode::Binary => instruction
                    .opcode
                    .binary_operator(instruction.flags)
                    .map_or_else(
                        || "Binary::<invalid>".to_owned(),
                        |operator| format!("Binary::{operator:?}"),
                    ),
                crate::ir::Opcode::Slow => instruction
                    .cold_variant
                    .map_or_else(|| "Slow".to_owned(), |variant| format!("Slow::{variant}")),
                opcode => format!("{opcode:?}"),
            };
            *inventory.entry(family).or_default() += 1;
        }
    }
    for (family, count) in inventory {
        println!("physical_inventory {family} {count}");
    }
}

fn emit_route_inventory(names: &[String]) {
    let mut inventory = std::collections::BTreeMap::<&'static str, RouteCount>::new();
    for name in names {
        let case = ExecutionCase::load(name);
        let (execution, profile) = crate::test_execution_profile::capture(|| execute_case(&case));
        execution.unwrap_or_else(|error| panic!("{name} route inventory failed: {error}"));
        for (route, count) in profile.stencils {
            let aggregate = inventory.entry(route).or_default();
            aggregate.entries = aggregate.entries.saturating_add(count.entries);
            aggregate.fallbacks = aggregate.fallbacks.saturating_add(count.fallbacks);
        }
    }
    for (route, count) in inventory {
        println!(
            "route_inventory {route} entries={} fallbacks={}",
            count.entries, count.fallbacks
        );
    }
}

fn emit_selected_mismatch() {
    let name = std::env::var(PROFILE_CASE_FILTER).expect("selected profile case");
    if let Some(mismatch) = case_mismatch(&name) {
        println!("{PROFILE_MISMATCH_MARKER}{mismatch}");
    }
}
