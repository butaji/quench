//! Derive callable obligations from oracle discovery and successful contract reports.
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs};

fn stdout(observation: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    match &observation["stdout"] {
        Value::String(text) => Ok(serde_json::from_str(text)?),
        _ => Err("expected textual surface observation".into()),
    }
}

// Contract tags describe public root functions, methods and accessors.
// A scenario pass without tags supplies no callable coverage.
fn contract_id(package: &str, function: &Value) -> Option<String> {
    let path = function["path"].as_str()?;
    let package = serde_json::to_string(package).ok()?;
    if path == format!("import({package})") || path == format!("require({package})") {
        return (function["kind"] == "call").then(String::new);
    }
    let (entry, selectors) = path.split_once(")[")?;
    if entry != format!("import({package}") && entry != format!("require({package}") {
        return None;
    }
    let selectors = format!("[{selectors}");
    let mut keys = Vec::new();
    for selector in selectors.split("][") {
        let key: String =
            serde_json::from_str(selector.trim_start_matches('[').trim_end_matches(']')).ok()?;
        if !matches!(key.as_str(), "default" | "module.exports") {
            if key.is_empty()
                || !key.chars().enumerate().all(|(index, ch)| {
                    ch.is_ascii_alphabetic()
                        || matches!(ch, '_' | '$')
                        || (index > 0 && ch.is_ascii_digit())
                })
            {
                return None;
            }
            keys.push(key);
        }
    }
    if keys.ends_with(&["prototype".into(), "constructor".into()]) {
        keys.truncate(keys.len() - 2);
    }
    let mut id = keys.join(".");
    let kind = function["kind"].as_str()?;
    if kind != "call" {
        id.push(':');
        id.push_str(kind);
    }
    Some(id)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let [surface_path, scenario_path, output] = args.as_slice() else {
        return Err("usage: npm-coverage SURFACE_REPORT SCENARIO_REPORT OUTPUT".into());
    };
    let surface: Value = serde_json::from_slice(&fs::read(surface_path)?)?;
    let scenario: Value = serde_json::from_slice(&fs::read(scenario_path)?)?;
    if surface["mode"] != "surface"
        || scenario["mode"] != "scenario"
        || surface["inventory_sha256"] != scenario["inventory_sha256"]
        || surface["lock_sha256"] != scenario["lock_sha256"]
        || surface["node_sha256"] != scenario["node_sha256"]
        || surface["quench_sha256"] != scenario["quench_sha256"]
    {
        return Err("coverage inputs must use the same inventory, lock and executable pair".into());
    }
    let scenarios: BTreeMap<_, _> = scenario["records"]
        .as_array()
        .ok_or("missing scenario records")?
        .iter()
        .map(|record| (record["package"]["name"].as_str().unwrap_or(""), record))
        .collect();
    let mut packages = Vec::new();
    for record in surface["records"]
        .as_array()
        .ok_or("missing surface records")?
    {
        let name = record["package"]["name"]
            .as_str()
            .ok_or("missing package name")?;
        let case = scenarios.get(name).ok_or("missing package scenario")?;
        let observed = stdout(&record["node"]);
        let contract = &case["node_function_checks"];
        let contract_pass = case["verdict"] == "behavior-pass"
            && contract.is_object()
            && contract["package"] == name
            && contract == &case["quench_function_checks"];
        let functions = observed
            .as_ref()
            .ok()
            .and_then(|v| v["functions"].as_array());
        let obligations: Vec<_> = functions
            .into_iter()
            .flatten()
            .map(|function| {
                let id = contract_id(name, function);
                let checked = contract_pass
                    && id
                        .as_ref()
                        .is_some_and(|id| contract["checks"][id].as_u64().is_some_and(|n| n > 0));
                json!({ "function": function, "contract_id": id,
                "status": if checked { "behavior-checked" } else { "uncovered" } })
            })
            .collect();
        let checked = obligations
            .iter()
            .filter(|v| v["status"] == "behavior-checked")
            .count();
        let requirements = json!({
            "discovered_callable_contracts": observed.is_ok() && checked == obligations.len(),
            "surface_parity": record["verdict"] == "surface-match-unqualified",
            "upstream_and_dynamic_api_contracts": false
        });
        let qualified = requirements
            .as_object()
            .unwrap()
            .values()
            .all(|met| met == true);
        packages.push(
            json!({ "package": name, "version": record["package"]["version"],
            "surface_verdict": record["verdict"], "scenario_verdict": case["verdict"],
            "discovered_callable_paths": obligations.len(), "behavior_checked_paths": checked,
            "uncovered_paths": obligations.len() - checked, "functions": obligations,
            "discovery_available": observed.is_ok(),
            "requirements": requirements,
            "oracle_surface": observed.ok(), "qualified": qualified }),
        );
    }
    let qualified =
        !packages.is_empty() && packages.iter().all(|package| package["qualified"] == true);
    let report = json!({ "qualified": qualified,
        "scope": "Observed exported functions, methods and accessors. Behavior-checked means the retained contract corpus ran on both engines. Dynamic returned APIs, upstream suites, declarations and CLI coverage remain separate obligations; descriptor matches never prove behavior.",
        "surface_report": surface_path, "scenario_report": scenario_path,
        "packages": packages });
    fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    if qualified {
        Ok(())
    } else {
        Err("function-level qualification incomplete; see coverage obligations".into())
    }
}
