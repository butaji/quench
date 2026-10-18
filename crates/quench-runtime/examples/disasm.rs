fn main() {
    let path = std::env::args().nth(1).expect("script");
    let source = std::fs::read_to_string(&path).unwrap();
    let program = quench_runtime::Engine::specialize(&source, &path).unwrap_or_else(|d| panic!("{d:?}"));
    print!("{}", program.disassemble());
}
