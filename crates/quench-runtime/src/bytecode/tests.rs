use super::*;

#[test]
fn generated_js_denotations_cover_the_native_numeric_lowerings() {
    assert_eq!(Op::LoadConst.js_denotation(), JsDenotation::LoadConst);
    assert_eq!(Op::Move.js_denotation(), JsDenotation::Move);
    assert_eq!(
        Op::LoadLocalPlain.js_denotation(),
        JsDenotation::PlainLocalLoad
    );
    assert_eq!(
        Op::StoreLocalPlain.js_denotation(),
        JsDenotation::PlainLocalStore
    );
    assert_eq!(Op::GetField.js_denotation(), JsDenotation::GetField);
    assert_eq!(Op::LoadCapture.js_denotation(), JsDenotation::CapturedValue);
    assert_eq!(Op::Binary.js_denotation(), JsDenotation::NumericBinary);
    assert_eq!(Op::NumericAdd.js_denotation(), JsDenotation::NumericBinary);
    assert_eq!(Op::NumericMultiply.js_denotation(), JsDenotation::NumericBinary);
    assert_eq!(Op::Unary.js_denotation(), JsDenotation::NumericUnary);
    assert_eq!(Op::Return.js_denotation(), JsDenotation::Return);
    assert_eq!(Op::Call.js_denotation(), JsDenotation::Unsupported);
}

#[test]
fn promoted_parameter_registers_survive_residual_round_trip() {
    let program = crate::Engine::specialize(
        "function read(value) { return value + 1; }",
        "promoted-registers.js",
    )
    .unwrap();
    let path =
        std::env::temp_dir().join(format!("quench-promoted-registers-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let decoded = ResidualProgram::read_binary(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(
        decoded.function(1).local_registers,
        program.function(1).local_registers
    );
    decoded.validate().unwrap();
}

#[test]
fn eval_binding_declarations_survive_residual_round_trip() {
    let source = r#"
    function outer(parameter) {
        try { throw 1; } catch (caught) {
            return function inner() { let lexical = 2; eval('parameter + caught + lexical'); };
        }
    }
    { let blocked = 3; with ({blocked: 4}) {
        blocked; blocked = 5; typeof blocked; delete blocked;
        let nested = 6; eval('nested');
    } }
"#;
    let program = crate::Engine::specialize(source, "eval-bindings.js").unwrap();
    let path = std::env::temp_dir().join(format!("quench-eval-bindings-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let encoded = std::fs::read(&path).unwrap();
    let decoded = ResidualProgram::read_binary(&path).unwrap();
    decoded.write_binary(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), encoded);
    std::fs::remove_file(path).unwrap();
    let declarations = decoded
        .functions
        .iter()
        .flat_map(|function| {
            function
                .name_bindings
                .resolved()
                .into_iter()
                .chain(
                    function
                        .binding_sites
                        .iter()
                        .flat_map(|site| site.bindings.iter().copied()),
                )
                .map(|binding| {
                    (
                        decoded.atoms[binding.atom as usize].as_ref(),
                        binding.declaration,
                    )
                })
        })
        .collect::<Vec<_>>();
    for (name, expected) in [
        ("parameter", EvalBindingDeclaration::Variable),
        ("caught", EvalBindingDeclaration::CatchParameter),
        ("lexical", EvalBindingDeclaration::Lexical),
        ("inner", EvalBindingDeclaration::Lexical),
        ("blocked", EvalBindingDeclaration::Lexical),
    ] {
        assert!(
            declarations.contains(&(name, expected)),
            "{name}: {declarations:?}"
        );
    }
    let nested = decoded
        .atoms
        .iter()
        .position(|atom| atom == "nested")
        .unwrap() as Atom;
    assert!(
        decoded
            .functions
            .iter()
            .flat_map(|function| {
                function
                    .binding_sites
                    .iter()
                    .flat_map(|site| &site.bindings)
            })
            .any(|binding| binding.atom == nested && binding.with_depth == 1)
    );
    let mut invalid = decoded.clone();
    let binding = invalid
        .functions
        .iter_mut()
        .flat_map(|function| {
            function
                .binding_sites
                .iter_mut()
                .flat_map(|site| &mut site.bindings)
        })
        .find(|binding| binding.atom == nested)
        .unwrap();
    binding.with_depth = u16::MAX;
    assert!(invalid.validate().is_err());
    assert!(EvalBindingDeclaration::from_binary_tag(u8::MAX).is_err());
}

#[test]
fn decoder_rejects_out_of_range_local_load() {
    let program = ResidualProgram {
        specialized: true,
        kind: crate::bytecode::ProgramKind::Script,
        module_requests: Vec::new(),
        module_imports: Vec::new(),
        module_link_plan: None,
        source_name: String::new(),
        source: None,
        atoms: AtomTable::default(),
        constants: vec![],
        functions: vec![Function {
            parent: None,
            name: None,
            inferred_name: None,
            is_arrow: false,
            self_binding_slot: None,
            source_span: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![].into(),
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            instance_initializer: None,
            super_home_atom: None,
            constructible: true,
            class_initializer: false,
            parameter_eval_arguments_error: false,
            unaliased_arguments: false,
            prototype_unobservable: false,
            arguments_slot: None,
            simple_parameters: true,
            strict: false,
            locals: 1,
            local_atoms: vec![].into(),
            environment_atoms: vec![].into(),
            selective_capture_slots: None,
            local_registers: Vec::new().into(),
            inherited_with_scope: false,
            lexical_atoms: vec![].into(),
            global_declarations: None,
            name_bindings: Default::default(),
            binding_sites: vec![].into(),
            source_positions: Default::default(),
            environment_clones: vec![].into(),
            code: vec![Instr::new(Op::LoadLocal, 0, 0, 0, 1)],
            wide: vec![],
            registers: 1,
            dispatch: DispatchClass::General,
            decoded: Default::default(),
            plain_locals: Default::default(),
            declared_atom_sets: Default::default(),
            activation_bindings: Default::default(),
            live_parameter_reflection: Default::default(),
            static_name_scope: Default::default(),
            js_tier_candidate: Default::default(),
            call_plan: Default::default(),
            handlers: vec![].into(),
            register_roots: Default::default(),
            undefined_register: None,
            parameter_registers: None,
            initial_register: Default::default(),
        }.into()],
        cache_sites: 0,
        method_sites: vec![],
        method_arguments: vec![],
        field_sites: vec![],
        object_sites: vec![],
        regexp_literal_sites: vec![],
        literal_templates: Vec::new(),
        superinstructions: vec![],
        replay: None,
        encoded_records: None,
            validated: Default::default(),
    };
    let path = std::env::temp_dir().join(format!("quench-invalid-local-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let result = ResidualProgram::read_binary(&path);
    std::fs::remove_file(path).unwrap();
    assert_eq!(result.unwrap_err(), "invalid residual local load");
}

#[test]
fn decoder_rejects_runtime_abi_mismatch_before_tables() {
    let program = ResidualProgram {
        specialized: true,
        kind: crate::bytecode::ProgramKind::Script,
        module_requests: Vec::new(),
        module_imports: Vec::new(),
        module_link_plan: None,
        source_name: String::new(),
        source: None,
        atoms: AtomTable::default(),
        constants: vec![],
        functions: vec![Function {
            parent: None,
            name: None,
            inferred_name: None,
            is_arrow: false,
            self_binding_slot: None,
            source_span: None,
            params: 0,
            length: 0,
            parameter_end_pc: 0,
            parameter_atoms: vec![].into(),
            rest: false,
            is_async: false,
            is_generator: false,
            is_class_constructor: false,
            derived_constructor: false,
            instance_initializer: None,
            super_home_atom: None,
            constructible: true,
            class_initializer: false,
            parameter_eval_arguments_error: false,
            unaliased_arguments: false,
            prototype_unobservable: false,
            arguments_slot: None,
            simple_parameters: true,
            strict: false,
            locals: 0,
            local_atoms: vec![].into(),
            environment_atoms: vec![].into(),
            selective_capture_slots: None,
            local_registers: Vec::new().into(),
            inherited_with_scope: false,
            lexical_atoms: vec![].into(),
            global_declarations: None,
            name_bindings: Default::default(),
            binding_sites: vec![].into(),
            source_positions: Default::default(),
            environment_clones: vec![].into(),
            code: vec![Instr::new(Op::Return, 0, 0, 0, 0)],
            wide: vec![],
            registers: 1,
            dispatch: DispatchClass::General,
            decoded: Default::default(),
            plain_locals: Default::default(),
            declared_atom_sets: Default::default(),
            activation_bindings: Default::default(),
            live_parameter_reflection: Default::default(),
            static_name_scope: Default::default(),
            js_tier_candidate: Default::default(),
            call_plan: Default::default(),
            handlers: vec![].into(),
            register_roots: Default::default(),
            undefined_register: None,
            parameter_registers: None,
            initial_register: Default::default(),
        }.into()],
        cache_sites: 0,
        method_sites: vec![],
        method_arguments: vec![],
        field_sites: vec![],
        object_sites: vec![],
        regexp_literal_sites: vec![],
        literal_templates: Vec::new(),
        superinstructions: vec![],
        replay: None,
        encoded_records: None,
            validated: Default::default(),
    };
    let path = std::env::temp_dir().join(format!("quench-invalid-abi-{}", std::process::id()));
    program.write_binary(&path).unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    let abi_bytes = ResidualProgram::RUNTIME_ABI_FINGERPRINT.to_le_bytes();
    let abi_offset = bytes
        .windows(abi_bytes.len())
        .position(|window| window == abi_bytes)
        .expect("serialized residual contains its runtime ABI fingerprint");
    bytes[abi_offset] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    let result = ResidualProgram::read_binary(&path);
    std::fs::remove_file(path).unwrap();
    assert_eq!(result.unwrap_err(), "residual runtime ABI mismatch");
}

#[test]
fn decoder_rejects_unknown_property_definition_modes() {
    for wide in [false, true] {
        let mut program = crate::Engine::specialize(
            "class Check {method() {return 42;}}",
            "invalid-definition-mode.js",
        )
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "quench-invalid-definition-mode-{}-{wide}",
            std::process::id()
        ));
        program.write_binary(&path).unwrap();
        ResidualProgram::read_binary(&path).unwrap();
        let function: &mut Function = program
            .functions
            .iter_mut()
            .find(|f| f.code.iter().any(|i| i.op() == Op::DefinePropertyRecord))
            .unwrap();
        let instruction = function
            .code
            .iter_mut()
            .find(|i| i.op() == Op::DefinePropertyRecord)
            .unwrap();
        let invalid = u32::try_from(PropertyDefinitionMode::ALL.len()).unwrap();
        if wide {
            let index = function.wide.len();
            function.wide.push(WideInstruction::new(
                Op::DefinePropertyRecord,
                instruction.a(),
                instruction.b(),
                instruction.c(),
                invalid,
            ));
            *instruction = Instr::wide(index).unwrap();
        } else {
            *instruction = Instr::new(
                Op::DefinePropertyRecord,
                instruction.a(),
                instruction.b(),
                instruction.c(),
                invalid,
            );
        }
        program.write_binary(&path).unwrap();
        let result = ResidualProgram::read_binary(&path);
        std::fs::remove_file(path).unwrap();
        assert!(
            result
                .unwrap_err()
                .contains("DefinePropertyRecord has an out-of-domain operand")
        );
    }
}

#[test]
fn instance_initializer_plan_survives_round_trip_and_rejects_invalid_owners() {
    let program = crate::Engine::specialize(
        "class Base {} class Derived extends Base {field=1; constructor(){(()=>super())();}}",
        "instance-initializer.js",
    )
    .unwrap();
    let owner = program
        .functions
        .iter()
        .position(|f| f.instance_initializer.is_some())
        .unwrap();
    let initializer = program.function(owner).instance_initializer.unwrap();
    let path = std::env::temp_dir().join(format!(
        "quench-instance-initializer-{}",
        std::process::id()
    ));
    program.write_binary(&path).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let decoded = ResidualProgram::read_binary(&path).unwrap();
    assert_eq!(
        decoded.function(owner).instance_initializer,
        Some(initializer)
    );
    decoded.write_binary(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    for plan in [program.functions.len() as u32, owner as u32, 0] {
        let mut invalid = program.clone();
        invalid.functions[owner].instance_initializer = Some(plan);
        invalid.write_binary(&path).unwrap();
        assert!(ResidualProgram::read_binary(&path).is_err());
    }
    let mut invalid = program;
    invalid.functions[initializer as usize].parent = Some(owner as u32);
    assert!(invalid.validate().is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn scoped_binding_sites_survive_residual_round_trip() {
    let source = "for (let x of [1]) { with ({}) { x++; } }";
    for generic in [false, true] {
        let program = if generic {
            crate::Engine::specialize_unspecialized(source, "binding-sites.js")
        } else {
            crate::Engine::specialize(source, "binding-sites.js")
        }
        .unwrap();
        let path = std::env::temp_dir().join(format!(
            "quench-binding-sites-{}-{generic}",
            std::process::id()
        ));
        program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(
            program
                .functions
                .iter()
                .any(|function| !function.binding_sites.is_empty())
        );
        for (before, after) in program.functions.iter().zip(&decoded.functions) {
            assert_eq!(before.binding_sites, after.binding_sites);
        }
        crate::Runtime::new(crate::SystemHost)
            .execute(&decoded)
            .unwrap();
    }
}

#[test]
fn decoder_rejects_invalid_binding_site_metadata() {
    let program = crate::Engine::specialize(
        "for (let x of [1]) { with ({}) { x++; } }",
        "binding-sites.js",
    )
    .unwrap();
    let path = std::env::temp_dir().join(format!(
        "quench-invalid-binding-sites-{}",
        std::process::id()
    ));
    for corrupt in 0..4 {
        let mut invalid = program.clone();
        let function = invalid
            .functions
            .iter_mut()
            .find(|f| !f.binding_sites.is_empty())
            .unwrap();
        match corrupt {
            0 => function.binding_sites[0].resume_pc = 0,
            1 => function.binding_sites[0].resume_pc = function.code.len() as u32 + 1,
            2 => {
                function.binding_sites[0].bindings[0].location =
                    EvalBindingLocation::Local(function.locals)
            }
            3 => {
                let mut sites = function.binding_sites.to_vec();
                sites.insert(0, sites[0].clone());
                function.binding_sites = sites.into();
            }
            _ => unreachable!(),
        }
        invalid.write_binary(&path).unwrap();
        let error = ResidualProgram::read_binary(&path).unwrap_err();
        assert!(
            error.contains("invalid binding sites")
                || error.contains("invalid name binding")
                || error.contains("invalid binding-site metadata"),
            "unexpected decoder error: {error}"
        );
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn fused_field_source_locations_survive_binary_round_trip_and_validate_coordinates() {
    // The stable Node oracle reports columns 87 and 89 for these two distinct gets.
    let source = "const positions=[];Error.prepareStackTrace=(_,frames)=>frames.find(site=>site.getFunctionName()==='read').getColumnNumber();\nconst leaf={get b(){positions.push(new Error().stack);return 1}};\nclass Holder{get a(){positions.push(new Error().stack);return leaf}read(){return this.a.b}}\nnew Holder().read();if(JSON.stringify(positions)!=='[87,89]')throw Error('lost field source locations');";
    for generic in [false, true] {
        let program = if generic {
            crate::Engine::specialize_unspecialized(source, "fused-field-binary.js")
        } else {
            crate::Engine::specialize(source, "fused-field-binary.js")
        }.unwrap();
        let path = std::env::temp_dir().join(format!("quench-fused-source-{}-{generic}", std::process::id()));
        program.write_binary(&path).unwrap();
        let decoded = ResidualProgram::read_binary(&path).unwrap();
        crate::Runtime::new(crate::SystemHost).execute(&decoded).unwrap();
        let site = program.field_sites.iter().position(|site| site.second.is_some()).expect("nested field site");
        for coordinate in [(0, 1), (1, 0)] {
            let mut invalid = program.clone();
            invalid.field_sites[site].sources[0] = Some(coordinate);
            invalid.write_binary(&path).unwrap();
            assert!(ResidualProgram::read_binary(&path).unwrap_err().contains("invalid field site"));
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn source_lines_share_javascript_boundaries_and_preserve_wrapper_origins() {
    let text = "a\r\nb\rc\nd\u{2028}e\u{2029}";
    assert_eq!(super::source_line_starts(text).collect::<Vec<_>>(), [0, 3, 5, 7, 11, 15]);
    let source = super::ProgramSource {
        text: super::SourceText::Shared(text.into()),
        base: 0,
        line_offset: 0,
        column_offset: 0,
        line_starts: Default::default(),
    };
    for (index, expected) in ["a", "b", "c", "d", "e", ""].into_iter().enumerate() {
        assert_eq!(source.source_line(index as u32 + 1), Some(expected));
    }
    assert_eq!(source.source_line(7), None);
    assert_eq!(source.source_line(0), None);
    let wrapped = super::ProgramSource {
        text: super::SourceText::Shared("wrapper\r\n😀first\rsecond\u{2028}third".into()),
        base: 0,
        line_offset: 1,
        column_offset: 2,
        line_starts: Default::default(),
    };
    assert_eq!(wrapped.source_line(1), Some("first"));
    assert_eq!(wrapped.source_line(2), Some("second"));
    assert_eq!(wrapped.source_line(3), Some("third"));
    // A static body kept without its wrapper prefix starts at the wrapper origin.
    let prefix = "(function () {\n  ";
    let body = "first\nsecond";
    let (line_offset, column_offset) = crate::wrapper_origin(prefix).unwrap();
    let retained = super::ProgramSource {
        text: super::SourceText::Static(body),
        base: prefix.len() as u32,
        line_offset,
        column_offset,
        line_starts: Default::default(),
    };
    assert_eq!(retained.source_line(1), Some("first"));
    assert_eq!(retained.source_line(2), Some("second"));
    let second = (prefix.len() + body.find("second").unwrap()) as u32;
    assert_eq!(retained.slice(second, second + 6), Some("second"));
    assert_eq!(retained.slice(0, second), None);
}

#[test]
fn source_positions_round_trip_every_delta_shape() {
    let positions = [
        SourcePosition { pc: 0, line: 3, column: 40 },
        SourcePosition { pc: 1, line: 3, column: 7 },
        SourcePosition { pc: 9, line: 1, column: 200_000 },
        SourcePosition { pc: 300, line: 1, column: 200_001 },
        SourcePosition { pc: u32::MAX, line: u32::MAX, column: u32::MAX },
    ];
    let table = positions.into_iter().collect::<SourcePositions>();
    assert_eq!(table.iter().collect::<Vec<_>>(), positions);
    assert_eq!(table.last(), positions.last().copied());
    assert_eq!(table.at(8), Some(positions[1]));
    assert_eq!(table.at(300), Some(positions[3]));
}

/// A compact encoding names every atom by its text: decoded under ids its reader's VM gives,
/// here each original id shifted past names the reader already holds, the program encodes
/// to the same bytes. The shift keeps the atoms' order, so atom-sorted bindings keep theirs,
/// and a field the encoding misses would be written with its shifted id instead.
#[test]
fn compact_encoding_names_every_atom_by_its_text() {
    let source = r#"
        var shared = 1; let lexical = 2;
        function outer(first, { nested: { inner } }, ...rest) {
            const object = { alpha: first, beta: [1, 2, 3], gamma() { return this.alpha; } };
            object.alpha.toString();
            object.delta = object.beta.length + inner;
            with (object) { alpha; }
            eval("first + lexical");
            return `${object.gamma()}-${rest.join(",")}`;
        }
        class Shape { field = 1; static count = 0; get area() { return this.field; } method() { return Shape.count; } }
        const arrow = (value) => value?.property ?? /pattern/g.source;
        function* sequence() { yield shared; }
    "#;
    let program = crate::Engine::specialize(source, "compact-atoms.js").unwrap();
    let (bytes, order) = program.encode_compact();
    let names: Vec<std::rc::Rc<str>> =
        order.iter().map(|&atom| std::rc::Rc::from(&program.atoms[atom as usize])).collect();
    let original: std::collections::HashMap<&str, u32> =
        names.iter().map(|name| &**name).zip(order.iter().copied()).collect();
    assert_eq!(original.len(), order.len(), "the test source names each atom once");
    const SHIFT: u32 = 1 << 12;
    let mut table = vec![std::rc::Rc::<str>::from(""); (SHIFT as usize) + program.atoms.len()];
    for (name, &atom) in names.iter().zip(&order) {
        table[(atom + SHIFT) as usize] = name.clone();
    }
    let text = super::SourceText::Shared(std::sync::Arc::from(source));
    let mut decoded =
        ResidualProgram::decode_compact(std::rc::Rc::new(bytes.clone()), &mut |name| original[name] + SHIFT, text).unwrap();
    decoded.atoms = super::AtomTable::from_rcs(&table);
    // Every record but the root's stays encoded until its first read; validation reads all.
    assert!(decoded.functions[0].record().is_some());
    assert!(decoded.functions[1..].iter().all(|slot| slot.record().is_none()));
    decoded.validate().unwrap();
    assert_eq!(decoded.encode_compact().0, bytes);
    // Ids past a narrow immediate widen the instructions naming them, as compiling does.
    const WIDE_SHIFT: u32 = 1 << 17;
    let text = super::SourceText::Shared(std::sync::Arc::from(source));
    let widened = ResidualProgram::decode_compact(std::rc::Rc::new(bytes.clone()), &mut |name| original[name] + WIDE_SHIFT, text).unwrap();
    widened.compile_released();
    let narrow: usize = decoded.functions.iter().map(|function| function.wide.len()).sum();
    let wide: usize = widened.functions.iter().map(|function| function.wide.len()).sum();
    assert!(wide > narrow, "atom ids past a narrow immediate are widened");
    let names = |program: &ResidualProgram, shift: u32| {
        program.functions.iter().flat_map(|function| {
            function.code.iter().map(move |instruction| {
                let instruction = super::control_flow::instruction_at(function, *instruction).unwrap();
                (instruction.op(), instruction.op().immediate_names_atom(instruction.b()).then(|| instruction.imm() - shift))
            })
        }).collect::<Vec<_>>()
    };
    assert_eq!(names(&decoded, SHIFT), names(&widened, WIDE_SHIFT));
    // The load checks the body; each deferred record is checked as it decodes.
    let encoding = &bytes[..bytes.len() - size_of::<u64>()];
    let region = super::binary::region_start(encoding).unwrap();
    let mut corrupted = bytes.clone();
    corrupted[region / 2] ^= 1;
    let text = super::SourceText::Shared(std::sync::Arc::from(source));
    assert!(ResidualProgram::decode_compact(std::rc::Rc::new(corrupted), &mut |name| original[name], text).is_err());
    let mut corrupted = bytes.clone();
    corrupted[region] ^= 1;
    let text = super::SourceText::Shared(std::sync::Arc::from(source));
    let lazy = ResidualProgram::decode_compact(std::rc::Rc::new(corrupted), &mut |name| original[name] + SHIFT, text)
        .expect("a deferred record is not read at load");
    let records = lazy.encoded_records.as_deref().unwrap();
    let first_deferred = (0..lazy.functions.len()).find(|&index| records.ranges[index].is_some_and(|record| record.start as usize == region)).unwrap();
    assert!(super::binary::decode_deferred_function(records, first_deferred).unwrap().is_err());
    let shifted = |program: &ResidualProgram| {
        program.functions.iter().filter_map(|function| function.name).collect::<Vec<_>>()
    };
    assert_eq!(
        shifted(&decoded),
        shifted(&program).iter().map(|atom| atom + SHIFT).collect::<Vec<_>>()
    );
}

