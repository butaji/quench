//! An ahead lane body (`ahead_images`) published in a VM and run as the interpreter would.
use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::AheadImages;
    use crate::test_host::TestHost;

    #[test]
    fn an_ahead_lane_body_runs_as_the_interpreter_does() {
        let program = crate::Engine::specialize(
            "var o = {x: 7}; function get(p){return p.x;} function add(a,b){return a+b;}", "ahead-body.js").unwrap();
        let mut vm = Vm::new(TestHost::default());
        vm.execute(&program).unwrap();
        let find = |name: &str| program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
        let (get, add) = (find("get"), find("add"));
        let (get_body, add_body) = (program.function(get), program.function(add));
        let file = AheadImages::compile(&program).to_bytes();
        vm.load_ahead_images(AheadImages::from_bytes(&file).unwrap());
        let sum = vm.install_ahead_body(&program, add as u32, None, &[]).expect("an ahead body for add");
        let sum = sum.lease().expect("published");
        let mut registers = vec![Value::UNDEFINED; usize::from(add_body.registers)];
        registers[0] = Value::number(2.0);
        registers[1] = Value::number(3.5);
        let pc = vm.run_lane_method(&*sum, false, super::super::super::program_store::ProgramId::MAIN, &program,
            registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
        let instruction = add_body.instructions().nth(pc).unwrap();
        assert_eq!(instruction.op(), crate::bytecode::Op::Return);
        assert_eq!(registers[usize::from(instruction.register_a())], Value::number(5.5));
        let read = vm.install_ahead_body(&program, get as u32, None, &[]).expect("an ahead body for get");
        let read = read.lease().expect("published");
        let atom = vm.intern_atom("o");
        let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
        let mut registers = vec![Value::UNDEFINED; usize::from(get_body.registers)];
        registers[0] = receiver;
        let pc = vm.run_lane_method(&*read, false, super::super::super::program_store::ProgramId::MAIN, &program,
            registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
        // No run observed `p`'s shape, so an ahead body has no field record to read it by: it
        // leaves at the read, with its inputs intact, and the interpreter finishes the call.
        assert_eq!(get_body.instructions().nth(pc).unwrap().op(), crate::bytecode::Op::GetField);
        assert_eq!(registers[0], receiver);
    }
}
