#[path = "../../crates/quench-node/src/host/hmr/type_worker_protocol.rs"]
mod protocol;

fn main() {
    let _ = (protocol::PROTOCOL_VERSION, protocol::TYPESCRIPT_VERSION);
    println!("cargo:rerun-if-changed=Cargo.lock");
    println!("cargo:rerun-if-env-changed=TSRS_TS_VERSION");
    let typescript_version = std::env::var("TSRS_TS_VERSION")
        .unwrap_or_else(|_| protocol::TYPESCRIPT_VERSION.to_owned());
    assert_eq!(
        typescript_version,
        protocol::TYPESCRIPT_VERSION,
        "update the shared HMR type-worker protocol when changing the TypeScript pin"
    );
    let lock = std::fs::read_to_string("Cargo.lock")
        .expect("the pinned checker worker must have a Cargo.lock");
    let source = lock
        .lines()
        .find(|line| line.starts_with("source = \"git+https://github.com/oxc-project/tsrs?"))
        .expect("Cargo.lock must contain the pinned TSRS source");
    let actual_revision = source
        .rsplit_once('#')
        .map(|(_, revision)| revision.trim_end_matches('"'))
        .expect("TSRS lock source must include its resolved revision");
    assert_eq!(
        actual_revision,
        protocol::TSRS_REVISION,
        "update the shared HMR type-worker protocol when changing the TSRS pin"
    );
}
