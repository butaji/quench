//! Separately linked Test262 qualification, preserving the existing stage and
//! metadata authorities. Durable variant records precede aggregate summaries.
use quench_test262::{discover_js_files, resolve_stages, TestMetadata};
use serde_json::{Value, json};
use std::{env, fs, path::PathBuf, process::ExitCode, time::Duration};
#[path = "run-aot-stages/measurement.rs"]
mod measurement;

const COMPILE_TIMEOUT: Duration = Duration::from_secs(300);
const EXECUTE_TIMEOUT: Duration = Duration::from_secs(30);
const REPORT_SCHEMA: u32 = 2;

fn main() -> ExitCode {
    match run() { Ok(()) => ExitCode::SUCCESS, Err(error) => { eprintln!("{error}"); ExitCode::FAILURE } }
}

fn run() -> Result<(), String> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [root, emitter, sdk, output, from, to, rounds, trailing @ ..] = args.as_slice() else {
        return Err("usage: run-aot-stages ROOT EMITTER SDK_REPORT OUTPUT FROM TO ROUNDS [--dynamic-source]".into());
    };
    let dynamic = match trailing { [] => false, [flag] if flag == "--dynamic-source" => true, _ => return Err("unknown AOT stage option".into()) };
    let from: u32 = from.parse().map_err(|_| "invalid first stage")?;
    let to: u32 = to.parse().map_err(|_| "invalid last stage")?;
    let rounds: usize = rounds.parse().map_err(|_| "invalid measurement rounds")?;
    if from > to || rounds == 0 { return Err("stage range and measurement rounds must be nonempty".into()); }
    let canonical = |path: &str| fs::canonicalize(path).map_err(|e| e.to_string());
    let root = canonical(root)?; let emitter = canonical(emitter)?; let sdk = canonical(sdk)?;
    let output = PathBuf::from(output);
    fs::create_dir(&output).map_err(|e| e.to_string())?;
    let output = fs::canonicalize(output).map_err(|e| e.to_string())?;
    let stages = resolve_stages(&root)?;
    let selected = stages.iter().filter(|stage| stage.id >= from && stage.id <= to).collect::<Vec<_>>();
    if selected.is_empty() { return Err("stage range contains no stages".into()); }
    let mut summaries = Vec::new();
    for stage in selected {
        let files = discover_js_files(&stage.root)?.into_iter().filter(|file| stage.owns_file(file, &stages)).collect::<Vec<_>>();
        let stage_root = output.join(format!("stage-{}",stage.id));
        fs::create_dir(&stage_root).map_err(|e| e.to_string())?;
        let files = files.into_iter().map(|path| {
            let original = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            Ok((path,TestMetadata::parse(&original)?))
        }).collect::<Result<Vec<_>,String>>()?;
        let plan = files.iter().map(|(path,metadata)| json!({"path":path,"variants":metadata.variants().iter().map(|variant|variant.name()).collect::<Vec<_>>()})).collect::<Vec<_>>();
        fs::write(stage_root.join("plan.json"),serde_json::to_vec_pretty(&plan).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        let mut records = Vec::new();
        for (index, (path,metadata)) in files.iter().enumerate() {
            for variant in metadata.variants() {
                let case_root = stage_root.join(format!("{index}-{}",variant.name()));
                fs::create_dir(&case_root).map_err(|e| e.to_string())?;
                let image_root = case_root.join("image");
                let mut arguments = vec![path.to_string_lossy().into_owned(),image_root.to_string_lossy().into_owned(),variant.name().into(),root.join("harness").to_string_lossy().into_owned(),"--sdk-report".into(),sdk.to_string_lossy().into_owned()];
                if dynamic { arguments.push("--dynamic-source".into()); }
                let compilation = measurement::run(&emitter,&arguments,&case_root,"compile",COMPILE_TIMEOUT,false)?;
                let compiled = compilation["success"] == true;
                let binary = image_root.join("native-test");
                let mut observations = Vec::new();
                if compiled {
                    for round in 0..rounds {
                        observations.push(measurement::run(&binary,&[],&case_root,&format!("run-{round}"),EXECUTE_TIMEOUT,true)?);
                    }
                }
                let memory = if compiled { Some(measurement::run_with_tree(&binary,&[],&case_root,"memory",EXECUTE_TIMEOUT,true)?) } else { None };
                let memory_valid = memory.as_ref().is_some_and(|record|record["success"]==true
                    && record["sampled_process_tree"]["observed_samples"].as_u64().is_some_and(|samples|samples>0));
                let passed = compiled && !compilation["maximum_resident_bytes"].is_null() && observations.len() == rounds
                    && memory_valid && observations.iter().all(|run| run["success"] == true && !run["maximum_resident_bytes"].is_null());
                let record = json!({"path":path.strip_prefix(root.join("test")).map_err(|e| e.to_string())?,"stage":stage.id,"variant":variant.name(),
                    "negative_phase":metadata.negative_phase,"negative_type":metadata.negative_type,"compilation":compilation,"observations":observations,
                    "memory_observation":memory,
                    "binary_bytes":fs::metadata(&binary).ok().map(|m|m.len()),"outcome":if passed {"pass"} else {"fail"},
                    "link_report":fs::read_to_string(image_root.join("link-report.json")).ok().and_then(|s|serde_json::from_str::<Value>(&s).ok())});
                fs::write(case_root.join("record.json"),serde_json::to_vec_pretty(&record).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
                println!("stage={} file={}/{} variant={} outcome={}",stage.id,index+1,files.len(),variant.name(),if passed {"pass"} else {"fail"});
                records.push(record);
            }
        }
        let passed = records.iter().filter(|r|r["outcome"]=="pass").count();
        let summary = json!({"schema":REPORT_SCHEMA,"stage":stage.id,"path":stage.path,"files":files.len(),"variants":records.len(),"passed":passed,"failed":records.len()-passed,
            "rounds":rounds,"dynamic_source":dynamic,"emitter":emitter,"sdk_report":sdk,"records":records,
            "scope":"Separately linked variants; timing includes startup and OS time but excludes tree sampling. Separate memory observation reports a sampled process-tree resident sum, a lower bound. No matched JIT/Node performance claim. Compile failures, including unsupported typed negative emission, remain failures."});
        fs::write(stage_root.join("report.json"),serde_json::to_vec_pretty(&summary).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        summaries.push(json!({"stage":stage.id,"files":files.len(),"variants":summary["variants"],"passed":passed,"failed":summary["failed"]}));
        fs::write(output.join("stages.json"),serde_json::to_vec_pretty(&summaries).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        if files.is_empty() || summary["failed"] != 0 { return Err(format!("stage {} did not pass its linked AOT correctness gate",stage.id)); }
    }
    Ok(())
}
