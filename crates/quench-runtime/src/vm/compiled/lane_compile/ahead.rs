//! A lane body compiled ahead (`LaneHelpers::Ahead`): from the program alone, with no fact a
//! run observed, to an image that links against the process helper table and publishes as an
//! ordinary lane method.
use super::*;
use crate::vm::jit_backend::relocatable_code::RelocatableCode;

impl<H: Host> Vm<H> {
    /// `function`'s lane body compiled ahead, linked here and published.
    pub(in crate::vm) fn compile_lane_method_ahead(&mut self, program: &ResidualProgram, function: u32) -> Option<LaneMethod> {
        let body = program.function_get(function as usize)?;
        let unit = lane_unit::with_budget(program, function, body, 0, &|_, _| Vec::new(), &|callee| program.function_get(callee as usize))?;
        let facts = VmFacts::STATIC;
        let mut lowered = lane_region::lower_body(program, function, &unit, LaneHelpers::Ahead, &BTreeMap::new(),
            &Default::default(), facts, &[], &BTreeMap::new(), &[], None, &[])?;
        let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
        let plan = LanePlan { unit, lowered, fields: BTreeMap::new(), direct: Box::new([]), closed: false, function,
            invalidation: crate::code_dependencies::Invalidation::default(), budget: 0 };
        let isa = super::super::jit_backend::target_isa()?;
        let image = super::super::jit_backend::compile_function(native, &*isa, Some(lane_region::LANE_BODY_MAX_CODE_BYTES), |compiled, function| {
            RelocatableCode::extract(compiled, function).ok_or(NativeCompileRejection::Relocations)
        }).ok()?;
        let table = LaneHelper::addresses::<H>();
        let linked = image.link(0, |namespace, key| table.get(lane_region::relocs::helper_key(namespace, key)?).map(|address| *address as usize as u64)).ok()?;
        plan.finish(&linked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        let sum = vm.compile_lane_method_ahead(&program, add as u32).expect("an ahead body for add");
        let mut registers = vec![Value::UNDEFINED; usize::from(add_body.registers)];
        registers[0] = Value::number(2.0);
        registers[1] = Value::number(3.5);
        let pc = vm.run_lane_method(&sum, false, super::super::super::program_store::ProgramId::MAIN, &program,
            registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
        let instruction = add_body.instructions().nth(pc).unwrap();
        assert_eq!(instruction.op(), crate::bytecode::Op::Return);
        assert_eq!(registers[usize::from(instruction.register_a())], Value::number(5.5));
        let read = vm.compile_lane_method_ahead(&program, get as u32).expect("an ahead body for get");
        let atom = vm.intern_atom("o");
        let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
        let mut registers = vec![Value::UNDEFINED; usize::from(get_body.registers)];
        registers[0] = receiver;
        let pc = vm.run_lane_method(&read, false, super::super::super::program_store::ProgramId::MAIN, &program,
            registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
        // No run observed `p`'s shape, so an ahead body has no field record to read it by: it
        // leaves at the read, with its inputs intact, and the interpreter finishes the call.
        assert_eq!(get_body.instructions().nth(pc).unwrap().op(), crate::bytecode::Op::GetField);
        assert_eq!(registers[0], receiver);
    }
}
