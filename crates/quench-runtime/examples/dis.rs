fn main() {
    let path = std::env::args().nth(1).expect("path");
    let source = std::fs::read_to_string(&path).unwrap();
    let program = quench_runtime::Engine::specialize(&source, "dis.js").unwrap_or_else(|_| panic!("compile"));
    println!("{}", program.disassemble());
}
