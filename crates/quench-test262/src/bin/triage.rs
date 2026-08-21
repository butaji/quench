#[path = "triage/support.rs"]
mod triage_support;
use std::{path::PathBuf, process::ExitCode, sync::{atomic::{AtomicUsize, Ordering}, Arc}, thread};
use quench_test262::{discover_js_files, HarnessCache, RuntimeHost, Test262Runner, TestOutcome};
use triage_support::*;

fn main() -> ExitCode {
    let args = match parse_args(std::env::args_os()) { Ok(a) => a, Err(e) => return fail(&e) };
    let root = test262_root(); let base = root.join("test").join(&args.target);
    let discovered = match discover_js_files(&base) { Ok(f) => f, Err(e) => return fail(&format!("discover: {e}")) };
    let discovered_count = discovered.len();
    let files: Vec<_> = select_files(discovered, &base, &args.filters).into_iter().filter(|p| !CRASHED_AT_RUNTIME.iter().any(|n| p.to_string_lossy().contains(n))).collect();
    if files.is_empty() { return fail("no tests matched the requested filters") }
    let sources = match load_test_sources(&files) { Ok(s) => s, Err(e) => return fail(&e) };
    println!("selected={} discovered={discovered_count}", files.len());
    let threads = args.threads.max(1).min(files.len());
    let (passed, failed, failures, outcomes) = run_parallel(&root, sources, args.limit, threads, args.json.is_some());
    if let Some(path) = &args.json { if let Err(e) = write_json_report(path, &args, &root, passed, failed, &outcomes) { return fail(&e) } }
    print_report(passed, failed, &bucket_failures(failures));
    if failed == 0 { ExitCode::SUCCESS } else { ExitCode::from(1) }
}
fn run_parallel(root:&std::path::Path, files:Vec<TestSource>, limit:usize, threads:usize, emit:bool)->(usize,usize,Vec<(PathBuf,String)>,Vec<JsonOutcome>){
    let files=Arc::new(files);let next=Arc::new(AtomicUsize::new(0));let count=Arc::new(AtomicUsize::new(0));let handles=(0..threads).map(|_|{let(f,n,c)=(files.clone(),next.clone(),count.clone());let r=root.to_path_buf();thread::Builder::new().stack_size(worker_stack_size()).spawn(move||run_worker(f,r,limit,c,n,emit)).expect("spawn triage worker")}).collect::<Vec<_>>();let mut out=RunReport::default();for h in handles{let x=h.join().unwrap_or_default();out.passed+=x.passed;out.failed+=x.failed;out.failures.extend(x.failures);out.outcomes.extend(x.outcomes)}out.failures.sort_by(|a,b|a.0.cmp(&b.0));out.outcomes.sort_by(|a,b|a.path.cmp(&b.path));(out.passed,out.failed,out.failures,out.outcomes)
}
fn run_worker(files:Arc<Vec<TestSource>>,root:PathBuf,limit:usize,count:Arc<AtomicUsize>,next:Arc<AtomicUsize>,emit:bool)->RunReport{let mut runner=Test262Runner::new(RuntimeHost);let mut harness=HarnessCache::new(root.join("harness"));let mut report=RunReport::default();loop{let start=next.fetch_add(WORK_BATCH,Ordering::Relaxed);if start>=files.len(){break}let stop=(start+WORK_BATCH).min(files.len());for fixture in &files[start..stop]{if count.load(Ordering::Relaxed)>=limit{break}let result=if fixture.metadata.is_module{runner.run_test_with_cache_metadata_and_path(&fixture.source,&fixture.metadata,&fixture.path,&mut harness)}else{runner.run_test_with_cache_and_metadata(&fixture.source,&fixture.metadata,&mut harness)};record_result(fixture,result,emit,&count,&mut report)}if count.load(Ordering::Relaxed)>=limit{break}}report}
fn record_result(fixture:&TestSource,result:Result<TestOutcome,String>,emit:bool,count:&Arc<AtomicUsize>,report:&mut RunReport){let(category,reason)=match result{Ok(TestOutcome::Pass)=>("pass".into(),None),Ok(TestOutcome::Fail{reason})|Err(reason)=>(normalize_reason(reason.trim()),Some(reason))};if emit{report.outcomes.push(JsonOutcome{path:fixture.path.clone(),category})}match reason{None=>report.passed+=1,Some(reason)=>{count.fetch_add(1,Ordering::Relaxed);report.failed+=1;report.failures.push((fixture.path.clone(),reason.trim().into()))}}}
