use std::{env, fs, path::PathBuf};

fn main() {
    generate_op_names();
    println!("cargo:rustc-check-cfg=cfg(quench_production)");
    println!("cargo:rerun-if-env-changed=PROFILE");
    let profile = env::var("PROFILE").unwrap_or_else(|_| "unknown".to_owned());
    // Keep this mapping exhaustive: a profile not represented here must not
    // silently masquerade as a production artifact.
    let lto = match profile.as_str() {
        "production" | "release" => "fat",
        "release-thin" => "thin",
        "debug" | "unknown" => "off",
        other => panic!("unsupported Cargo profile for quench runtime: {other}"),
    };
    let production = matches!(profile.as_str(), "release" | "production" | "release-thin");
    if production {
        println!("cargo:rustc-cfg=quench_production");
    }
    println!("cargo:rustc-env=QUENCH_BUILD_PROFILE={profile}");
    println!("cargo:rustc-env=QUENCH_BUILD_LTO={lto}");
}

fn generate_op_names() {
    const PREFIX: &str = "    ";
    let source = fs::read_to_string("src/ops_op.rs").expect("read canonical Op declaration");
    let body = source
        .split_once("pub enum Op {")
        .expect("Op declaration")
        .1;
    let variants: Vec<_> = body
        .lines()
        .take_while(|line| *line != "}")
        .filter_map(|line| {
            let rest = line.strip_prefix(PREFIX)?;
            if rest.starts_with(' ') {
                return None;
            }
            let end = rest
                .find(|character: char| !character.is_ascii_alphanumeric() && character != '_')?;
            let name = &rest[..end];
            (!name.is_empty() && name.chars().next()?.is_ascii_uppercase()).then_some((name, rest))
        })
        .collect();
    assert!(variants.len() >= 90, "incomplete Op variant extraction");
    let arms = variants
        .iter()
        .map(|(name, declaration)| {
            let pattern = if declaration[name.len()..].trim_start().starts_with('{') {
                format!("Self::{name} {{ .. }}")
            } else if declaration[name.len()..].trim_start().starts_with('(') {
                format!("Self::{name} (..)")
            } else {
                format!("Self::{name}")
            };
            format!("            {pattern} => \"{name}\",")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let generated = format!(
        "impl Op {{\n    pub const fn variant_name(&self) -> &'static str {{\n        match self {{\n{arms}\n        }}\n    }}\n}}\n"
    );
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    fs::write(output.join("op_variant_name.rs"), generated).expect("write Op names");
    println!("cargo:rerun-if-changed=src/ops_op.rs");
}
