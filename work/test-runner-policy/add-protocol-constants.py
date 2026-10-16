from pathlib import Path
r=Path('/workspace/quench/work/prepared-realm-worktree')
def edit(n,a,b):
 p=r/n;s=p.read_text();assert a in s,n;p.write_text(s.replace(a,b))
edit('crates/quench-node/build.rs','    configure_node_compat_version(&root);','    configure_node_compat_version(&root);\n    embed_node_protocol_constants(&root);')
p=r/'crates/quench-node/build.rs';s=p.read_text();s+='''
/// Read Node's actual protocol enum owners; never maintain a second numeric table.
fn embed_node_protocol_constants(root: &Path) {
    fn entries(root: &Path, file: &str, name: &str) -> Vec<String> {
        let path = root.join("src").join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let source = std::fs::read_to_string(path).expect("read Node protocol constants");
        let definition = format!("#define {name}(V)");
        let mut lines = source.lines().skip_while(|line| !line.starts_with(&definition));
        assert!(lines.next().is_some(), "Node protocol macro {name}");
        let mut entries = Vec::new();
        for line in lines {
            let line = line.trim();
            if let Some(entry) = line.strip_prefix("V(") {
                entries.push(entry.split_once(')').expect("Node constant entry").0.to_owned());
            }
            if !line.ends_with('\\\\') { break; }
        }
        assert!(!entries.is_empty(), "Node protocol constants {name}");
        entries
    }
    let codes = entries(root, "node_exit_code.h", "EXIT_CODE_LIST");
    let statuses = entries(root, "compile_cache.h", "COMPILE_CACHE_STATUS");
    let mut output = String::from("pub(crate) const EXIT_CODES: &[(&str, i32)] = &[\\n");
    for code in codes {
        let (name, value) = code.split_once(',').expect("Node exit code pair");
        let value: i32 = value.trim().parse().expect("numeric Node exit code");
        output.push_str(&format!("    ({:?}, {value}),\\n", format!("k{}", name.trim())));
    }
    output.push_str("];\\npub(crate) const COMPILE_CACHE_STATUS: &[&str] = &[\\n");
    for status in statuses { output.push_str(&format!("    {status:?},\\n")); }
    output.push_str("];\\n");
    let path = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("node_protocol_constants.rs");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(&output) {
        std::fs::write(path, output).expect("write Node protocol constants");
    }
}
''';p.write_text(s)
p=r/'crates/quench-node/src/modules/node_protocol_constants.rs';p.write_text('//! Protocol enums derived at build time from the configured Node SDK headers.\ninclude!(concat!(env!("OUT_DIR"), "/node_protocol_constants.rs"));\n')
edit('crates/quench-node/src/modules/mod.rs','pub(crate) mod node_errors_shared_vm;','pub(crate) mod node_errors_shared_vm;\npub(crate) mod node_protocol_constants;')
edit('crates/quench-node/src/modules/node_errors_shared_vm.rs','''    context.define_data_property_rooted(binding, key, function, true, true, true)?;
    Ok(binding)
}''','''    context.define_data_property_rooted(binding, key, function, true, true, true)?;
    let codes = context.object_rooted()?;
    for &(name, code) in super::node_protocol_constants::EXIT_CODES {
        let key = context.string_rooted(name);
        let value = context.number(f64::from(code));
        context.define_data_property_rooted(codes, key, value, false, true, false)?;
    }
    let key = context.string_rooted("exitCodes");
    context.define_data_property_rooted(binding, key, codes, false, true, true)?;
    Ok(binding)
}''')
edit('crates/quench-node/src/modules/trace_shared_vm.rs','''        "errors" => return super::node_errors_shared_vm::binding(context),''','''        "errors" => return super::node_errors_shared_vm::binding(context),
        "modules" => return crate::host::shared_vm::commonjs::cached_native_binding(
            context, "\\0binding:modules", module_protocol_binding,
        ),''')
p=r/'crates/quench-node/src/modules/trace_shared_vm.rs';s=p.read_text();s+='''
fn module_protocol_binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    let statuses = super::node_protocol_constants::COMPILE_CACHE_STATUS.iter()
        .map(|status| context.string_rooted(status)).collect::<Vec<_>>();
    let statuses = context.array_rooted(&statuses)?;
    put(context, binding, "compileCacheStatus", statuses)?;
    Ok(binding)
}
''';p.write_text(s)
edit('crates/quench-node/src/host/shared_vm/commonjs.rs','    case "errors": return __nativeInternalBinding(name);','    case "errors":\n    case "modules": return __nativeInternalBinding(name);')
