//! Matched declared-variant process measurements. Timing and sampled tree
//! memory are separate experiments; neither rewrites guest/harness inputs.
use serde_json::{Value,json};
use std::{env,fs,path::PathBuf,process::ExitCode,time::Duration};
#[path = "run-aot-stages/measurement.rs"]
mod measurement;
const TIMEOUT:Duration = Duration::from_secs(30);

fn main()->ExitCode { match run() { Ok(())=>ExitCode::SUCCESS,Err(error)=>{ eprintln!("{error}");ExitCode::FAILURE } } }
fn run()->Result<(),String> {
    let args=env::args().skip(1).collect::<Vec<_>>();
    let [suite,stage,jit_report,node,oracle,output,rounds]=args.as_slice() else {
        return Err("usage: compare-aot-stage SUITE STAGE_REPORT JIT_CARGO_REPORT NODE NODE_ORACLE OUTPUT ROUNDS".into());
    };
    let rounds:usize=rounds.parse().map_err(|_|"invalid rounds")?;
    if rounds==0 { return Err("rounds must be positive".into()); }
    let canonical=|path:&str|fs::canonicalize(path).map_err(|e|e.to_string());
    let suite=canonical(suite)?;let node=canonical(node)?;let oracle=canonical(oracle)?;
    let stage_path=canonical(stage)?;let jit_report=canonical(jit_report)?;
    let stage:Value=serde_json::from_slice(&fs::read(&stage_path).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    if stage["failed"]!=0 { return Err("AOT stage correctness gate has not passed".into()); }
    let artifacts=fs::read_to_string(&jit_report).map_err(|e|e.to_string())?.lines()
        .map(serde_json::from_str::<Value>).collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
    if !artifacts.iter().any(|row|row["reason"]=="build-finished" && row["success"]==true) { return Err("JIT build did not finish successfully".into()); }
    let jit=artifacts.iter().filter(|row|row["reason"]=="compiler-artifact" && row["target"]["name"]=="run-test" && row["profile"]["test"]==false).collect::<Vec<_>>();
    if jit.len()!=1 { return Err("JIT executable artifact is ambiguous or missing".into()); }
    let jit=jit[0];let executable=jit["executable"].as_str().ok_or("JIT executable path missing")?;
    let output=PathBuf::from(output);fs::create_dir(&output).map_err(|e|e.to_string())?;
    let output=fs::canonicalize(output).map_err(|e|e.to_string())?;
    let pinned_jit=output.join("quench-jit");fs::copy(executable,&pinned_jit).map_err(|e|e.to_string())?;
    let records=stage["records"].as_array().ok_or("AOT stage variant records missing")?;
    let mut results=Vec::new();
    for (index,record) in records.iter().enumerate() {
        let relative=record["path"].as_str().ok_or("test path missing")?;
        let variant=record["variant"].as_str().ok_or("variant missing")?;
        let aot=record["link_report"]["binary"].as_str().ok_or("AOT executable missing")?;
        let path=suite.join("test").join(relative);
        let config=vec![format!("TEST262_DIR={}",suite.display()),format!("TEST262_TEST_TIMEOUT_MS={}",TIMEOUT.as_millis())];
        let mut aot_command=config.clone();aot_command.push(aot.into());
        let mut jit_command=config.clone();jit_command.extend([pinned_jit.to_string_lossy().into_owned(),"--jit".into(),"--variant".into(),variant.into(),path.to_string_lossy().into_owned()]);
        let mut node_command=config;node_command.extend([node.to_string_lossy().into_owned(),"--experimental-vm-modules".into(),oracle.to_string_lossy().into_owned(),relative.into(),variant.into()]);
        let engines=[("aot",aot_command),("jit",jit_command),("node",node_command)];
        let directory=output.join(index.to_string());fs::create_dir(&directory).map_err(|e|e.to_string())?;
        let mut setup=Vec::new();
        for (engine,command) in &engines { setup.push(json!({"engine":engine,"observation":measurement::run(std::path::Path::new("/usr/bin/env"),command,&directory,&format!("setup-{engine}"),TIMEOUT,true)?})); }
        let mut timing=Vec::new();
        for round in 0..rounds {
            for position in 0..engines.len() {
                let (engine,command)=&engines[(index+round+position)%engines.len()];
                timing.push(json!({"round":round,"position":position,"engine":engine,"observation":measurement::run(std::path::Path::new("/usr/bin/env"),command,&directory,&format!("timing-{round}-{engine}"),TIMEOUT,true)?}));
            }
        }
        let mut memory=Vec::new();
        for (engine,command) in &engines { memory.push(json!({"engine":engine,"observation":measurement::run_with_tree(std::path::Path::new("/usr/bin/env"),command,&directory,&format!("memory-{engine}"),TIMEOUT,true)?})); }
        let passed=setup.iter().chain(&timing).chain(&memory).all(|row|row["observation"]["success"]==true)
            && memory.iter().all(|row|row["observation"]["sampled_process_tree"]["observed_samples"].as_u64().is_some_and(|samples|samples>0));
        let result=json!({"path":relative,"variant":variant,"passed":passed,"setup":setup,"timing":timing,"memory":memory});
        fs::write(directory.join("record.json"),serde_json::to_vec_pretty(&result).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        println!("variant={}/{} {} {} passed={}",index+1,records.len(),relative,variant,passed);
        results.push(result);
    }
    let passed=results.iter().all(|row|row["passed"]==true) && results.len()==records.len() && !results.is_empty();
    let production=jit["profile"]["opt_level"]=="3" && records.iter().all(|row|row["link_report"]["support_artifacts"]["optimization_level"]=="3");
    let report=json!({"schema":1,"passed":passed,"stage_report":stage_path,"jit_cargo_report":jit_report,"jit_artifact":jit,"node":node,"oracle":oracle,"rounds":rounds,"production_optimization":production,"records":results,
        "scope":"Same declared variants through AOT/shared JIT/Node, rotating engine order. Fresh-process startup-inclusive timing; premeasurement setup separate. Memory observation separate from timing, sampled resident sum is a lower bound. No steady-state throughput claim; production optimization is a prerequisite, not sufficient performance qualification."});
    fs::write(output.join("report.json"),serde_json::to_vec_pretty(&report).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    if passed { Ok(()) } else { Err("matched comparison failed correctness or memory observation".into()) }
}
