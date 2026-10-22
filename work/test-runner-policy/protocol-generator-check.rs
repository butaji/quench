use std::{path::Path,env};
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
            if !line.ends_with('\\') { break; }
        }
        assert!(!entries.is_empty(), "Node protocol constants {name}");
        entries
    }
    let codes = entries(root, "node_exit_code.h", "EXIT_CODE_LIST");
    let statuses = entries(root, "compile_cache.h", "COMPILE_CACHE_STATUS");
    let mut output = String::from("pub(crate) const EXIT_CODES: &[(&str, i32)] = &[\n");
    for code in codes {
        let (name, value) = code.split_once(',').expect("Node exit code pair");
        let value: i32 = value.trim().parse().expect("numeric Node exit code");
        output.push_str(&format!("    ({:?}, {value}),\n", format!("k{}", name.trim())));
    }
    output.push_str("];\npub(crate) const COMPILE_CACHE_STATUS: &[&str] = &[\n");
    for status in statuses { output.push_str(&format!("    {status:?},\n")); }
    output.push_str("];\n");
    let path = std::path::PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("node_protocol_constants.rs");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(&output) {
        std::fs::write(path, output).expect("write Node protocol constants");
    }
}

fn main(){embed_node_protocol_constants(Path::new("/workspace/quench/tests/node"));}
