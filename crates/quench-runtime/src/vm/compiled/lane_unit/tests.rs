use super::*;

#[cfg(feature = "aot")]
#[test]
fn static_hints_keep_numeric_local_indices_out_of_register_rows() {
    // Captured prefix bindings separate the local and register index spaces;
    // the loop closure keeps the caught-result local in its frame slot.
    let names: Vec<_> = (0..16).map(|index| format!("prefix{index}")).collect();
    let bindings = names.iter().map(|name| format!("{name}=0")).collect::<Vec<_>>().join(",");
    let source = format!("function check(marker){{let {bindings};function observe(){{return [{}];}}for(const item of [1]){{let capture=()=>item;let caught=false;try{{throw item;}}catch(error){{caught=error===marker;}}consume(caught,capture);}}return observe;}}", names.join(","));
    for typescript in [false, true] {
        let program = if typescript { crate::Engine::specialize_typescript(&source, "local-hints.cts") }
            else { crate::Engine::specialize(&source, "local-hints.cjs") }.unwrap();
        program.validate().unwrap();
        assert!(program.functions.iter().any(|function| function.instructions().any(|instruction|
            instruction.numeric_local_target().is_some_and(|local|
                local >= function.registers && local < function.locals))),
            "the regression must exercise a valid local outside the register row:\n{}", program.disassemble());
        let targets = static_call_targets(&program);
        assert_eq!(targets.len(), program.function_count());
        for (function, targets) in program.functions.iter().zip(targets) {
            assert_eq!(targets.len(), function.code.len());
        }
    }
}

#[cfg(feature = "aot")]
#[test]
fn method_hints_follow_definition_records_and_bound_conflicting_bodies() {
    // Each case names how many bodies the call site is hinted with: every distinct
    // definition of the name, each guarded at the call, up to `tier_policy::POLYMORPHISM`.
    for (definition, expected) in [
        ("class Thing { probe(){return 1;} }", 1),
        ("class Thing { static probe(){return 1;} }", 1),
        ("let mask=4;class Thing { probe(){return (this.bits&mask)!==0||this.bits===2;} }", 1),
        // Computed keys travel through ToPropertyKey and environment
        // homes; this literal-register proof deliberately declines them.
        ("class Thing { ['probe'](){return 1;} }", 0),
        ("function make(mask){return class Thing { probe(){return mask|0;} };}", 1),
        ("class Thing { probe(){return 1;} probe(){return 2;} }", 2),
        ("class First { probe(){return 1;} } class Second { probe(){return 2;} }", 2),
        ("class Thing { probe(){return 1;} } Thing.prototype.probe=function replacement(){return 2;};", 2),
        (&(0..=crate::vm::compiled::tier_policy::POLYMORPHISM).map(|n| format!("class C{n} {{ probe(){{return {n};}} }}")).collect::<String>(), 0),
        ("class Thing { get probe(){return function(){return 1;};} }", 0),
        ("class Thing { set probe(value){} }", 0),
        ("class Thing { get probe(){return 1;} set probe(value){} }", 0),
        ("class Thing { [unknownName](){return 1;} }", 0),
    ] {
        let source = format!("{definition} function caller(receiver){{let result=receiver.probe();return result;}}");
        for typescript in [false, true] {
            let admitted = if typescript { crate::Engine::specialize_typescript(&source, "method-hints.cts") }
                else { crate::Engine::specialize(&source, "method-hints.cjs") };
            let program = match admitted {
                Ok(program) => program,
                Err(errors) if typescript && expected != 1 => {
                    // OXC rejects duplicate TypeScript implementations;
                    // JavaScript's accepted duplicate still exercises the
                    // conflicting-definition path below.
                    assert!(errors.iter().any(|error|
                        error.syntax_message().contains("Identifier `probe` has already been declared")), "{definition}: {errors:?}");
                    continue;
                }
                Err(errors) => panic!("{definition}: {errors:?}"),
            };
            let caller = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "caller")).unwrap();
            let targets = static_call_targets(&program);
            assert_eq!(targets[caller].iter().map(|site| site.len()).sum::<usize>(), expected,
                "{definition}\n{}", program.disassemble());
            if expected == 1 {
                let target = targets[caller].iter().flatten().copied().next().unwrap();
                assert!(program.function(target as usize).instructions().any(|instruction| instruction.op().class().eq(Class::Return)
                    || instruction.returns_from_frame()));
                let unit = unit(&program, caller as u32, program.function(caller), &|function, pc|
                    targets[function as usize].get(pc as usize).into_iter().flatten().copied().next()
                        .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
                    &|id| program.function_get(id as usize)).unwrap();
                assert_eq!(unit.frames.len(), 1, "method hint must reach the shared guarded inliner: {definition}");
                assert_eq!(unit.frames[0].target, CallTarget { function: target, callee: CalleeBinding::Function });
            }
        }
    }
}

#[cfg(feature = "aot")]
#[test]
fn captured_hints_resolve_lexical_depth_and_bound_conflicting_bodies() {
    for (binding, expected) in [
        ("let target = function leaf(x) {return x + 1;};", &["leaf"][..]),
        ("let target; if (flag) target = function first(x) {return x + 1;}; else target = function second(x) {return x + 2;};",
            &["first", "second"][..]),
    ] {
        let source = format!("function outer(flag) {{{binding} function middle() {{function caller(x) {{return target(x);}} return caller;}} return middle();}}");
        let program = crate::Engine::specialize(&source, "capture-depth.js").unwrap();
        let caller = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == "caller")).unwrap();
        assert!(program.function(caller).instructions().any(|instruction|
            instruction.op().is_load_capture() && instruction.capture_depth() == 1));
        let targets = static_call_targets(&program);
        let names: Vec<&str> = targets[caller].iter().flatten()
            .map(|target| &program.atoms[program.function(*target as usize).name.unwrap() as usize]).collect();
        assert_eq!(names, expected);
    }
}


#[cfg(feature = "aot")]
#[test]
fn terminal_leaf_uses_the_existing_caller_return_and_strict_calls_residualize() {
    for strict in [false, true] {
        let directive = if strict { "'use strict';" } else { "" };
        let source = format!("let p={{}};p.leaf=function(x){{this.count=this.count+1;return x+this.count;}};function run(o,x){{{directive}return o.leaf(x);}}");
        let program = crate::Engine::specialize(&source, "terminal-inline.js").unwrap();
        let caller = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
        let targets = static_call_targets(&program);
        let original_terminal = program.function(caller).code.len() - 1;
        let unit = unit(&program, caller as u32, program.function(caller), &|function, pc| targets[function as usize]
            .get(pc as usize).into_iter().flatten().copied().next()
            .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
            &|id| program.function_get(id as usize)).unwrap();
        if strict {
            assert!(unit.frames.is_empty(), "strict terminal calls preserve VM tail-call policy");
            continue;
        }
        assert_eq!(unit.frames.len(), 1);
        assert!(!unit.instructions.iter().any(|instruction| instruction.op().class().eq(Class::Call)));
        let terminal = unit.instructions.len() - 1;
        let returned = unit.instructions[terminal];
        assert_eq!(returned.op(), Op::Return);
        assert_eq!(unit.origins[terminal], Origin::Caller(original_terminal as u32),
            "completion must recover at the original Return, never the Call");
        assert!(unit.instructions.iter().any(|instruction| instruction.op().class().eq(Class::Move)
            && instruction.result_register() == returned.register_a()));
        assert!(unit.instructions.iter().any(|instruction| instruction.op().class().eq(Class::Jump)
            && instruction.jump_target() as usize == terminal));
        let live = unit.register_masks(&program, caller as u32).expect("terminal splice liveness");
        assert!(live.is_live(terminal, returned.register_a()));
    }
}

#[cfg(feature = "aot")]
#[test]
fn nested_strict_terminal_calls_residualize_in_their_own_caller() {
    for strict in [false, true] {
        let directive = if strict { "'use strict';" } else { "" };
        let source = format!("let p={{}};p.leaf=function(x){{return x+1;}};p.middle=function(o,x){{{directive}return o.leaf(x);}};function run(o,x){{let r=o.middle(o,x);return r+1;}}");
        let program = crate::Engine::specialize(&source, "nested-terminal-inline.js").unwrap();
        let caller = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
        let targets = static_call_targets(&program);
        let unit = unit(&program, caller as u32, program.function(caller), &|function, pc| targets[function as usize]
            .get(pc as usize).into_iter().flatten().copied().next()
            .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
            &|id| program.function_get(id as usize)).unwrap();
        if strict {
            assert!(unit.frames.is_empty(), "a nested strict terminal call must keep its VM tail-call policy");
        } else {
            assert_eq!(unit.frames.len(), 2, "ordinary nested terminal calls retain recursive expansion");
            assert_eq!(unit.frames[0].parent, None);
            assert_eq!(unit.frames[1].parent, Some(0));
            assert!(!unit.instructions.iter().any(|instruction| instruction.op().class().eq(Class::Call)));
        }
    }
}

#[cfg(feature = "aot")]
#[test]
fn function_binding_distinguishes_effectful_prefixes_from_pure_numeric_loops() {
    for (body, recovery) in [
        ("this.state=1;return this.next+1;", CallRecovery::Materialize),
        ("let x=0;while(x<4)x++;return x;", CallRecovery::RestartCall),
        ("let x=0;while(x<4)x++;this.state=x;return this.next+1;", CallRecovery::Materialize),
        ("this.next.value=1;return this.other+1;", CallRecovery::Materialize)] {
        let source = format!("let o={{}};o.unsafeProbe=function(){{{body}}};function run(o){{let r=o.unsafeProbe();if(r)return 11;return 12;}}");
        let program = crate::Engine::specialize(&source, "non-restartable.js").unwrap();
        let caller = program.functions.iter().position(|body|
            body.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
        let targets = static_call_targets(&program);
        assert!(targets[caller].iter().any(|site| !site.is_empty()), "the hint must reach admission");
        let unit = unit(&program, caller as u32, program.function(caller), &|function, pc| targets[function as usize]
            .get(pc as usize).into_iter().flatten().copied().next()
            .map(|function| CallTarget { function, callee: CalleeBinding::Function }).into_iter().collect(),
            &|id| program.function_get(id as usize)).unwrap();
        assert_eq!(unit.frames.len(), 1, "exact recovery admits {body}");
        assert_eq!(unit.frames[0].recovery, recovery, "{body}");
    }
}

#[test]
fn remapped_liveness_covers_synthesized_reads_without_loading_callee_registers_at_entry() {
    let program = crate::Engine::specialize(
        "function leaf(x) { return x + 1; } function caller(x) { return this.leaf(x) + 2; }",
        "inline-liveness.js",
    ).unwrap();
    let id = |name: &str| program.functions.iter().position(|function|
        function.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap() as u32;
    let caller = id("caller");
    let leaf = id("leaf");
    let body = program.function(caller as usize);
    let call = body.instructions().position(|instruction| instruction.op().class().eq(Class::Call)).unwrap();
    let mut targets = vec![None; body.code.len()];
    targets[call] = Some(CallTarget { callee: CalleeBinding::Identity(Value::UNDEFINED), function: leaf });
    let mut unit = unit(&program, caller, body, &|function, pc| if function == caller {
        targets.get(pc as usize).into_iter().flatten().copied().next().into_iter().collect()
    } else { Vec::new() }, &|function| program.function_get(function as usize)).unwrap();
    assert_eq!(unit.frames.len(), 1);
    let masks = unit.register_masks(&program, caller).expect("covered remapped graph");
    for register in unit.caller_registers..unit.registers {
        assert!(!masks.is_live(0, register), "callee register {register} is defined by call admission");
    }
    let (pc, instruction) = unit.instructions.iter().enumerate().find(|(pc, instruction)|
        instruction.op().class().eq(Class::Move) && matches!(unit.origins[*pc], Origin::Inlined { .. })
            && instruction.result_register() < unit.caller_registers).expect("synthesized result move");
    assert!(masks.is_live(pc, instruction.register_b()), "return projection must retain its produced result");
    drop(masks);
    unit.registers = crate::compile::liveness::ROOT_MAP_REGISTERS + 1;
    assert!(unit.register_masks(&program, caller).is_none(), "uncovered widths retain conservative recovery");
}
