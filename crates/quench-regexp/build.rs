//! Tabulates the inverse of case canonicalization for each mode (`canonical_preimages`).

include!("src/canonical.rs");

fn main() {
    println!("cargo:rerun-if-changed=src/canonical.rs");
    let mut tables = String::new();
    for (name, unicode) in [("CANONICAL_PREIMAGES_BMP", false), ("CANONICAL_PREIMAGES_UNICODE", true)] {
        let preimages = preimages(unicode);
        tables.push_str(&format!("static {name}: [(u32, u32); {}] = [", preimages.len()));
        for (form, member) in preimages {
            tables.push_str(&format!("({form},{member}),"));
        }
        tables.push_str("];\n");
    }
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    std::fs::write(out.join("canonical_preimages.rs"), tables).expect("OUT_DIR is writable");
}
