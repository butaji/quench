use quench_runtime::{
    ExecutionMode, NativeProgramNamespace, NativeSourceCompilation, ResidualProgram, SourceKind,
};
use quench_test262::{HarnessCache, TestMetadata, TestVariant};
use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};
fn emit(
    program: &ResidualProgram,
    index: usize,
    root: &Path,
    namespace: Option<&NativeProgramNamespace>,
    exports: &mut BTreeSet<String>,
) -> Result<(String, String, String), String> {
    program.freeze_native_diagnostics();
    let label = format!("test262_linked_{index}");
    let module = quench_runtime::lower_native_js_module(program, &label)?;
    let symbol = module.entry_table_symbol().to_owned();
    for f in &module.functions {
        if let quench_runtime::NativeFunction::Import { name, .. } = f {
            exports.insert(name.clone());
        }
    }
    fs::write(
        root.join(format!("unit_{index}.o")),
        quench_runtime::emit_native_module(&label, module.functions, module.data)?,
    )
    .map_err(|e| e.to_string())?;
    let metadata = root.join(format!("unit_{index}.bin"));
    if let Some(namespace) = namespace {
        fs::write(&metadata, namespace.encode_with_external_source(program)?)
            .map_err(|e| e.to_string())?;
    } else {
        program.write_binary(&metadata)?;
    }
    let source = if namespace.is_some() {
        if let Some(text) = program.source_text() {
            let path = root.join(format!("unit_{index}.source"));
            fs::write(&path, text).map_err(|e| e.to_string())?;
            format!("Some(include_str!({:?}))", path.to_str().unwrap())
        } else {
            "None".into()
        }
    } else {
        "None".into()
    };
    Ok((metadata.to_str().unwrap().into(), symbol, source))
}
fn main() -> Result<(), String> {
    std::thread::Builder::new()
        .name("test262-emit".into())
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(run)
        .map_err(|error| error.to_string())?
        .join()
        .map_err(|_| "native test compilation panicked".to_owned())?
}

fn run() -> Result<(), String> {
    let args = std::env::args().collect::<Vec<_>>();
    const USAGE: &str =
        "usage: emit-native CASE OUTPUT_DIRECTORY VARIANT HARNESS_DIRECTORY [--dynamic-source]";
    let dynamic_source = match args.len() {
        5 => false,
        6 if args[5] == "--dynamic-source" => true,
        _ => return Err(USAGE.into()),
    };
    let path = PathBuf::from(&args[1]);
    let root = PathBuf::from(&args[2]);
    fs::create_dir(&root).map_err(|e| e.to_string())?;
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let original = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut metadata = TestMetadata::parse(&original)?;
    let variants = metadata.variants();
    let variant = *variants
        .iter()
        .find(|v| v.name() == args[3])
        .ok_or("variant is not declared by test metadata")?;
    metadata.only_strict = variant == TestVariant::Strict;
    let harness_dir = PathBuf::from(args.get(4).ok_or("missing harness directory")?);
    let mut cache = HarnessCache::new(harness_dir);
    let harness = quench_test262::harness_sources(&metadata, &mut cache)?;
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let source = quench_test262::script_source(&original, metadata.only_strict);
    let mut host = quench_test262::Test262RuntimeHost::new(ExecutionMode::NativeOnly);
    let prepared = host.prepare_native_programs(
        &harness,
        &source,
        path.to_str().ok_or("test path must be UTF-8")?,
        if metadata.is_module {
            SourceKind::Module
        } else {
            SourceKind::Script
        },
    )?;
    let quench_test262::NativeCasePrograms {
        harness: scripts,
        modules,
        function_sources: functions,
        entry,
        entry_module,
    } = prepared;
    let namespace = NativeProgramNamespace::from_entry(&entry)?;
    let mut exports = BTreeSet::new();
    let (entry_metadata, entry_symbol, _) = emit(&entry, 0, &root, None, &mut exports)?;
    let mut catalog=format!("quench_runtime::linked_native_sources!{{host quench_test262::Test262RuntimeHost; entry bind_entry: entry_table => ({entry_symbol:?},{entry_metadata:?}); resolver resolve {{\n");
    let mut binders = String::new();
    let mut count = 1;
    for (i, script) in scripts.iter().enumerate() {
        let (meta, symbol, source) = emit(script, count, &root, Some(&namespace), &mut exports)?;
        let key = format!("<linked-harness:{i}>");
        writeln!(
            catalog,
            "unit_{count}_table => ({symbol:?},{meta:?},{key:?},[],source {source}),"
        )
        .unwrap();
        writeln!(binders,"unsafe{{resolve(&namespace,{key:?},&[])}}.map_err(source_error)?.ok_or(\"missing harness\")?,").unwrap();
        count += 1;
    }
    let mut module_binders = String::new();
    let mut source_binders = String::new();
    for (i, (module, program)) in modules.iter().enumerate() {
        let (meta, symbol, source) = emit(program, count, &root, Some(&namespace), &mut exports)?;
        let key = format!("<linked-module:{i}>");
        writeln!(
            catalog,
            "unit_{count}_table => ({symbol:?},{meta:?},{key:?},[],source {source}),"
        )
        .unwrap();
        writeln!(module_binders,"unsafe{{resolve(&namespace,{key:?},&[])}}.map_err(source_error)?.ok_or(\"missing module\")?,").unwrap();
        count += 1;
        let file = root.join(format!("module_{i}.input"));
        fs::write(&file, module.bytes()).map_err(|e| e.to_string())?;
        writeln!(
            source_binders,
            "quench_runtime::ModuleSource::from_bytes({:?},include_bytes!({:?}).to_vec()),",
            module.name,
            file.to_str().unwrap()
        )
        .unwrap();
    }
    if let Some(module) = entry_module {
        let file = root.join("entry.input");
        fs::write(&file, module.bytes()).map_err(|e| e.to_string())?;
        writeln!(
            source_binders,
            "quench_runtime::ModuleSource::from_bytes({:?},include_bytes!({:?}).to_vec()),",
            module.name,
            file.to_str().unwrap()
        )
        .unwrap();
    }
    let mut errors = String::new();
    for (key, compilation) in functions {
        let parts = key
            .parts()
            .map(|p| format!("{p:?}"))
            .collect::<Vec<_>>()
            .join(",");
        match compilation {
            NativeSourceCompilation::Program(program) => {
                let (meta, symbol, source) =
                    emit(&program, count, &root, Some(&namespace), &mut exports)?;
                writeln!(
                    catalog,
                    "unit_{count}_table => ({symbol:?},{meta:?},{:?},[{parts}],source {source}),",
                    key.name()
                )
                .unwrap();
                count += 1;
            }
            NativeSourceCompilation::SyntaxError(message) => {
                writeln!(errors, "({:?},[{parts}]) => {message:?},", key.name()).unwrap();
            }
        }
    }
    writeln!(catalog, "}} errors {{{errors}}}}}").unwrap();
    writeln!(
        catalog,
        "const _:()=assert!(quench_runtime::ResidualProgram::RUNTIME_ABI_FINGERPRINT=={:#x});",
        ResidualProgram::RUNTIME_ABI_FINGERPRINT
    )
    .unwrap();
    let operations = exports
        .iter()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(",");
    writeln!(catalog,"quench_runtime::export_native_js_operations!(quench_test262::Test262RuntimeHost,semantic_exports,[{operations}]);").unwrap();
    fs::write(root.join("catalog.rs"), catalog).map_err(|e| e.to_string())?;
    let option = |s: &Option<String>| {
        s.as_ref()
            .map(|s| format!("Some({s:?}.into())"))
            .unwrap_or("None".into())
    };
    let driver = format!(
        r#"
use quench_test262::Test262Host;
const _:()=assert!(if {dynamic_source} {{ quench_test262::NATIVE_SUPPORT_HAS_JIT && !quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY }} else {{ quench_test262::NATIVE_SUPPORT_IS_EXECUTION_ONLY && quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY }});
include!({catalog:?});
fn source_error(error:quench_runtime::NativeSourceError)->String{{format!("{{error:?}}")}}
fn run()->Result<(),String>{{
 let entry=unsafe{{bind_entry()}}?;let namespace=quench_runtime::NativeProgramNamespace::from_entry(entry.residual())?;
 let harness=vec![{binders}];
 let graph=quench_test262::LinkedCasePrograms{{harness,modules:vec![{module_binders}],entry,module_sources:vec![{source_binders}]}};
 let metadata=quench_test262::TestMetadata{{is_module:{is_module},is_async:{is_async},can_block:{can_block},only_strict:{strict},negative_phase:{phase},negative_type:{negative},..Default::default()}};
 let mut host=quench_test262::Test262RuntimeHost::new(if {dynamic_source} {{ quench_runtime::ExecutionMode::Compiled }} else {{ quench_runtime::ExecutionMode::NativeOnly }});host.configure(&metadata);
 let completion=host.execute_linked_programs(graph,move|name,parts|unsafe{{resolve(&namespace,name,parts)}});
 match quench_test262::classify_test_outcome(quench_test262::execution_outcome(completion),&metadata){{quench_test262::TestOutcome::Pass=>Ok(()),quench_test262::TestOutcome::Fail{{reason}}=>Err(reason)}}
}}
fn main()->std::process::ExitCode{{match run(){{Ok(())=>std::process::ExitCode::SUCCESS,Err(error)=>{{eprintln!("{{error}}");std::process::ExitCode::FAILURE}}}}}}
"#,
        dynamic_source = dynamic_source,
        catalog = root.join("catalog.rs").to_str().unwrap(),
        binders = binders,
        module_binders = module_binders,
        source_binders = source_binders,
        is_module = metadata.is_module,
        is_async = metadata.is_async,
        can_block = metadata.can_block,
        strict = metadata.only_strict,
        phase = option(&metadata.negative_phase),
        negative = option(&metadata.negative_type)
    );
    fs::write(root.join("driver.rs"), driver).map_err(|e| e.to_string())?;
    fs::write(
        root.join("required_exports.txt"),
        exports.iter().cloned().collect::<Vec<_>>().join("\n") + "\n",
    )
    .map_err(|error| error.to_string())?;
    let prefix = if cfg!(target_os = "macos") { "_" } else { "" };
    let exports_text = std::iter::once(format!("{prefix}main"))
        .chain(exports.into_iter().map(|name| format!("{prefix}{name}")))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(root.join("exports.txt"), exports_text).map_err(|e| e.to_string())?;
    fs::write(root.join("units.txt"), count.to_string()).map_err(|e| e.to_string())?;
    println!(
        "prepared {} {:?}: {} native units, original harness count {}",
        path.display(),
        variant,
        count,
        harness.len()
    );
    Ok(())
}
