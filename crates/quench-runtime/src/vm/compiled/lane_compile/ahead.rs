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

    /// The program's output with `images` loaded (none: the interpreter alone), the functions
    /// run hot enough that a compile would publish their bodies.
    fn output_with(source: &str, images: Option<&[u8]>) -> Vec<String> {
        let program = crate::Engine::specialize(source, "ahead-run.js").unwrap();
        let lines = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(lines.clone()));
        if let Some(file) = images {
            vm.set_execution_mode(crate::ExecutionMode::Compiled);
            vm.load_ahead_images(AheadImages::from_bytes(file).unwrap());
        }
        vm.execute(&program).unwrap();
        let lines = lines.borrow().clone();
        lines
    }

    /// An ahead-compiled numeric function and a property-reading function print what the
    /// interpreter prints, whichever of their guards leave for the generic path.
    #[test]
    fn ahead_compiled_numeric_and_property_functions_match_the_interpreter() {
        let source = "function mix(a,b){let t=0;for(let i=0;i<a;i=i+1){t=t+i*b-(i%3);}return t;}\n\
            function read(p){return p.x+p.y;}\n\
            let o={x:1.5,y:2};let total=0;\n\
            for(let i=0;i<3000;i=i+1){total=total+mix(10,0.5)+read(o);}\n\
            print(total);print(mix(7,3));print(read({x:'a',y:1}));print(read({y:4,x:6}));";
        let program = crate::Engine::specialize(source, "ahead-run.js").unwrap();
        let images = AheadImages::compile(&program);
        assert!(!images.is_empty(), "the numeric function has an ahead image");
        let expected = output_with(source, None);
        assert_eq!(expected.len(), 4);
        assert_eq!(output_with(source, Some(&images.to_bytes())), expected);
    }

    #[test]
    fn ahead_images_serve_only_the_program_they_were_compiled_from() {
        let program = crate::Engine::specialize("function add(a,b){return a+b;}", "ahead-a.js").unwrap();
        let other = crate::Engine::specialize("function add(a,b){return a-b;}", "ahead-b.js").unwrap();
        let file = AheadImages::compile(&program).to_bytes();
        for (served, expected) in [(&program, true), (&other, false)] {
            let mut vm = Vm::new(TestHost::default());
            vm.load_ahead_images(AheadImages::from_bytes(&file).unwrap());
            assert_eq!(vm.install_ahead_body(served, 1, None, &[]).is_some(), expected);
        }
    }

    /// An image file from another build (its header differs) is not read at all.
    #[test]
    fn images_of_another_build_are_rejected() {
        let program = crate::Engine::specialize("function add(a,b){return a+b;}", "ahead-a.js").unwrap();
        let mut file = AheadImages::compile(&program).to_bytes();
        assert!(AheadImages::from_bytes(&file).is_ok());
        file[8] ^= 1;
        assert!(AheadImages::from_bytes(&file).is_err());
    }

    /// A body that names atoms (a store) names them by symbol: the loading VM
    /// numbers them itself.
    #[test]
    fn an_ahead_body_resolves_the_atoms_it_names_in_the_loading_vm() {
        let source = "globalThis.o = {}; function put(p){p.value = 6; return p.value;}";
        let program = crate::Engine::specialize(source, "ahead-atoms.js").unwrap();
        let images = AheadImages::compile(&program);
        let file = images.to_bytes();
        let mut vm = Vm::new(TestHost::default());
        vm.execute(&program).unwrap();
        vm.load_ahead_images(AheadImages::from_bytes(&file).unwrap());
        let put = program.functions.iter().position(|body| body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "put")).unwrap();
        let method = vm.install_ahead_body(&program, put as u32, None, &[]).expect("an ahead body for put");
        let method = method.lease().expect("published");
        let atom = vm.intern_atom("o");
        let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
        let mut registers = vec![Value::UNDEFINED; usize::from(program.function(put).registers)];
        registers[0] = receiver;
        let pc = vm.run_lane_method(&*method, false, super::super::super::program_store::ProgramId::MAIN, &program,
            registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
        // Whether it ran to the return or left for the interpreter, the stored word is not corrupt.
        let value = vm.intern_atom("value");
        let stored = vm.own_property(receiver, value);
        assert!(stored.is_none() || stored == Some(Value::number(6.0)), "pc {pc}: {stored:?}");
    }
}
