//! Object publication for the same native IR consumed by the JIT.

use cranelift_codegen::ir;
use cranelift_module::{DataDescription, Linkage, Module, ModuleReloc};
use cranelift_object::{ObjectBuilder, ObjectModule};

/// Cranelift Module's function namespace. Indices refer to declaration order,
/// including imports; these are symbolic identities, never process addresses.
pub const NATIVE_FUNCTION_NAMESPACE: u32 = 0;

/// Cranelift Module's data namespace; indices refer to data declaration order.
pub const NATIVE_DATA_NAMESPACE: u32 = 1;

/// A pointer-sized function address, resolved by the system linker.
pub struct NativeFunctionAddress {
    pub offset: u32,
    /// Index in the module's function declaration order, including imports.
    pub function: u32,
}

/// Linker-owned program constants, assets, or writable native storage.
pub struct NativeData {
    pub name: String,
    pub bytes: Vec<u8>,
    pub alignment: u64,
    pub writable: bool,
    pub function_addresses: Vec<NativeFunctionAddress>,
}

/// A symbol's signature has one authority: its definition, or its import.
// A module holds one entry per symbol and emits each once; most are definitions, so
// boxing them would add an allocation per function to save space on a few imports.
#[allow(clippy::large_enum_variant)]
pub enum NativeFunction {
    Definition {
        name: String,
        function: ir::Function,
    },
    Import {
        name: String,
        signature: ir::Signature,
    },
}

/// Emit a host-native relocatable object without allocating executable memory.
/// All symbols are declared before compilation so recursion and forward calls
/// resolve through the same table. The system linker performs final placement.
pub fn emit_native_object(
    module_name: &str,
    functions: Vec<NativeFunction>,
) -> Result<Vec<u8>, String> {
    emit_native_module(module_name, functions, Vec::new())
}

/// Publish native functions and data together, resolving symbolic references
/// through their respective declaration namespaces before execution.
pub fn emit_native_module(
    module_name: &str,
    functions: Vec<NativeFunction>,
    data: Vec<NativeData>,
) -> Result<Vec<u8>, String> {
    let isa = super::jit_backend::target_isa_for(
        "speed",
        super::jit_backend::CodePublication::RelocatableObject,
    )
    .ok_or_else(|| "native target unavailable".to_owned())?;
    let builder = ObjectBuilder::new(
        isa.clone(),
        module_name,
        cranelift_module::default_libcall_names(),
    )
    .map_err(|error| error.to_string())?;
    let mut module = ObjectModule::new(builder);
    let mut symbols = Vec::with_capacity(functions.len());
    for function in &functions {
        let (name, signature, linkage) = match function {
            NativeFunction::Definition { name, function } => {
                (name, &function.signature, Linkage::Export)
            }
            NativeFunction::Import { name, signature } => (name, signature, Linkage::Import),
        };
        // Duplicate declarations would make declaration indices cease to name
        // their own symbols, so reject them even when signatures match.
        if module.get_name(name).is_some() {
            return Err(format!("duplicate native symbol: {name}"));
        }
        symbols.push(
            module
                .declare_function(name, linkage, signature)
                .map_err(|error| error.to_string())?,
        );
    }
    let mut data_symbols = Vec::with_capacity(data.len());
    for item in &data {
        if !item.alignment.is_power_of_two() {
            return Err(format!(
                "{}: native data alignment must be a nonzero power of two",
                item.name
            ));
        }
        if module.get_name(&item.name).is_some() {
            return Err(format!("duplicate native symbol: {}", item.name));
        }
        data_symbols.push(
            module
                .declare_data(&item.name, Linkage::Export, item.writable, false)
                .map_err(|error| error.to_string())?,
        );
    }
    for (index, definition) in functions.into_iter().enumerate() {
        let NativeFunction::Definition { name, function } = definition else {
            continue;
        };
        if function
            .dfg
            .ext_funcs
            .values()
            .any(|external| matches!(external.name, ir::ExternalName::TestCase(_)))
        {
            return Err(format!("{name}: test-only external names cannot be linked"));
        }
        for global in function.global_values.values() {
            if let ir::GlobalValueData::Symbol {
                name: target, tls, ..
            } = global
            {
                if matches!(target, ir::ExternalName::TestCase(_)) {
                    return Err(format!("{name}: test-only external names cannot be linked"));
                }
                if *tls {
                    return Err(format!(
                        "{name}: thread-local native symbols are not declared"
                    ));
                }
            }
        }
        for target in function.params.user_named_funcs().values() {
            let declared = match target.namespace {
                NATIVE_FUNCTION_NAMESPACE => (target.index as usize) < symbols.len(),
                NATIVE_DATA_NAMESPACE => (target.index as usize) < data_symbols.len(),
                _ => false,
            };
            if !declared {
                return Err(format!(
                    "{name}: undeclared native function or data {target}"
                ));
            }
        }
        for external in function.dfg.ext_funcs.values() {
            if let ir::ExternalName::User(reference) = external.name {
                let target = &function.params.user_named_funcs()[reference];
                if target.namespace != NATIVE_FUNCTION_NAMESPACE {
                    return Err(format!(
                        "{name}: data symbol {target} cannot be called as a function"
                    ));
                }
            }
        }
        let id = symbols[index];
        super::jit_backend::compile_function(function, &*isa, None, |code, function| {
            let relocations = code
                .buffer
                .relocs()
                .iter()
                .map(|relocation| ModuleReloc::from_mach_reloc(relocation, function, id))
                .collect::<Vec<_>>();
            Ok(module.define_function_bytes(
                id,
                u64::from(code.buffer.alignment),
                code.code_buffer(),
                &relocations,
            ))
        })
        .map_err(|error| format!("{name}: {}", error.reason()))?
        .map_err(|error| format!("{name}: {error}"))?;
    }
    for (id, item) in data_symbols.into_iter().zip(data) {
        let pointer_bytes = usize::from(isa.pointer_bytes());
        let mut offsets: Vec<_> = item
            .function_addresses
            .iter()
            .map(|address| address.offset)
            .collect();
        offsets.sort_unstable();
        if offsets
            .windows(2)
            .any(|pair| u64::from(pair[0]) + pointer_bytes as u64 > u64::from(pair[1]))
        {
            return Err(format!("{}: native function addresses overlap", item.name));
        }
        for address in &item.function_addresses {
            if address.function as usize >= symbols.len() {
                return Err(format!("{}: undeclared native function address", item.name));
            }
            if (address.offset as usize)
                .checked_add(pointer_bytes)
                .is_none_or(|end| end > item.bytes.len())
            {
                return Err(format!(
                    "{}: native function address exceeds data",
                    item.name
                ));
            }
        }
        let mut description = DataDescription::new();
        description.define(item.bytes.into_boxed_slice());
        description.set_align(item.alignment);
        for address in item.function_addresses {
            let function =
                module.declare_func_in_data(symbols[address.function as usize], &mut description);
            description.write_function_addr(address.offset, function);
        }
        module
            .define_data(id, &description)
            .map_err(|error| error.to_string())?;
    }
    module.finish().emit().map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cranelift_codegen::ir::{AbiParam, InstBuilder, types};
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

    const PROGRAM_EXIT_STATUS: i64 = 42;

    fn signature() -> ir::Signature {
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        let mut signature = ir::Signature::new(isa.default_call_conv());
        signature.returns.push(AbiParam::new(types::I32));
        signature
    }

    fn function(name: &str, target: Option<u32>) -> NativeFunction {
        let mut function = ir::Function::with_name_signature(
            ir::UserFuncName::user(NATIVE_FUNCTION_NAMESPACE, 0),
            signature(),
        );
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let entry = builder.create_block();
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let result = if let Some(target) = target {
            let signature = builder.import_signature(signature());
            let name = builder
                .func
                .declare_imported_user_function(ir::UserExternalName::new(
                    NATIVE_FUNCTION_NAMESPACE,
                    target,
                ));
            let target = builder.import_function(ir::ExtFuncData {
                name: ir::ExternalName::user(name),
                signature,
                colocated: false,
                patchable: false,
            });
            let call = builder.ins().call(target, &[]);
            builder.inst_results(call)[0]
        } else {
            builder.ins().iconst(types::I32, PROGRAM_EXIT_STATUS)
        };
        builder.ins().return_(&[result]);
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        builder.finalize(isa.frontend_config());
        NativeFunction::Definition {
            name: name.into(),
            function,
        }
    }

    #[test]
    fn object_links_and_runs_a_forward_symbolic_call() {
        let object = emit_native_object(
            "native-call",
            vec![function("main", Some(1)), function("answer", None)],
        )
        .unwrap();
        assert_eq!(link_and_run(object), Some(PROGRAM_EXIT_STATUS as i32));
    }

    #[test]
    fn object_links_and_loads_relocated_program_data() {
        let isa = super::super::jit_backend::target_isa("speed").unwrap();
        let mut main = ir::Function::with_name_signature(
            ir::UserFuncName::user(NATIVE_FUNCTION_NAMESPACE, 0),
            signature(),
        );
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut main, &mut context);
        let entry = builder.create_block();
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let name = builder
            .func
            .declare_imported_user_function(ir::UserExternalName::new(NATIVE_DATA_NAMESPACE, 0));
        let constant = builder.create_global_value(ir::GlobalValueData::Symbol {
            name: ir::ExternalName::user(name),
            offset: ir::immediates::Imm64::new(0),
            colocated: false,
            tls: false,
        });
        let pointer = builder.ins().symbol_value(isa.pointer_type(), constant);
        let answer = builder
            .ins()
            .load(types::I32, ir::MemFlagsData::trusted(), pointer, 0);
        builder.ins().return_(&[answer]);
        builder.finalize(isa.frontend_config());
        let object = emit_native_module(
            "native-data",
            vec![NativeFunction::Definition {
                name: "main".into(),
                function: main,
            }],
            vec![NativeData {
                name: "answer".into(),
                bytes: (PROGRAM_EXIT_STATUS as u32).to_ne_bytes().to_vec(),
                alignment: std::mem::align_of::<u32>() as u64,
                writable: false,
                function_addresses: Vec::new(),
            }],
        )
        .unwrap();
        assert_eq!(link_and_run(object), Some(PROGRAM_EXIT_STATUS as i32));
    }

    #[test]
    fn invalid_data_layout_and_symbol_kind_are_diagnostics() {
        for alignment in [0, 3] {
            let error = emit_native_module(
                "alignment",
                Vec::new(),
                vec![NativeData {
                    name: "data".into(),
                    bytes: vec![0],
                    alignment,
                    writable: false,
                    function_addresses: Vec::new(),
                }],
            )
            .unwrap_err();
            assert!(error.contains("alignment"), "{error}");
        }
        let error = emit_native_module(
            "duplicate",
            vec![function("main", None)],
            vec![NativeData {
                name: "main".into(),
                bytes: vec![0],
                alignment: 1,
                writable: false,
                function_addresses: Vec::new(),
            }],
        )
        .unwrap_err();
        assert!(error.contains("duplicate native symbol"), "{error}");

        let NativeFunction::Definition {
            function: mut main, ..
        } = function("main", Some(0))
        else {
            unreachable!()
        };
        let reference = main.params.user_named_funcs().keys().next().unwrap();
        main.params.reset_user_func_name(
            reference,
            ir::UserExternalName::new(NATIVE_DATA_NAMESPACE, 0),
        );
        let error = emit_native_module(
            "data-call",
            vec![NativeFunction::Definition {
                name: "main".into(),
                function: main,
            }],
            vec![NativeData {
                name: "data".into(),
                bytes: vec![0],
                alignment: 1,
                writable: false,
                function_addresses: Vec::new(),
            }],
        )
        .unwrap_err();
        assert!(error.contains("cannot be called as a function"), "{error}");
    }

    fn link_and_run(object: Vec<u8>) -> Option<i32> {
        let directory = std::env::temp_dir().join(format!(
            "quench-native-object-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir(&directory).unwrap();
        let object_path = directory.join("program.o");
        let executable = directory.join("program");
        std::fs::write(&object_path, object).unwrap();
        let link = std::process::Command::new("cc")
            .arg(&object_path)
            .arg("-o")
            .arg(&executable)
            .output()
            .unwrap();
        assert!(
            link.status.success(),
            "{}",
            String::from_utf8_lossy(&link.stderr)
        );
        let status = std::process::Command::new(&executable).status().unwrap();
        std::fs::remove_dir_all(directory).unwrap();
        status.code()
    }

    #[test]
    fn undeclared_and_duplicate_symbols_are_diagnostics() {
        let error = emit_native_object("missing", vec![function("main", Some(1))]).unwrap_err();
        assert!(error.contains("undeclared native function"), "{error}");
        let error = emit_native_object(
            "duplicate",
            vec![function("main", None), function("main", None)],
        )
        .unwrap_err();
        assert!(error.contains("duplicate native symbol"), "{error}");
    }

    #[test]
    fn invalid_function_address_relocations_are_diagnostics() {
        let pointer_bytes = std::mem::size_of::<usize>();
        for (addresses, diagnostic) in [
            (
                vec![NativeFunctionAddress {
                    offset: 0,
                    function: 1,
                }],
                "undeclared native function address",
            ),
            (
                vec![NativeFunctionAddress {
                    offset: pointer_bytes as u32,
                    function: 0,
                }],
                "exceeds data",
            ),
            (
                vec![
                    NativeFunctionAddress {
                        offset: 0,
                        function: 0,
                    },
                    NativeFunctionAddress {
                        offset: 1,
                        function: 0,
                    },
                ],
                "overlap",
            ),
        ] {
            let error = emit_native_module(
                "bad-address",
                vec![function("main", None)],
                vec![NativeData {
                    name: "table".into(),
                    bytes: vec![0; pointer_bytes],
                    alignment: pointer_bytes as u64,
                    writable: false,
                    function_addresses: addresses,
                }],
            )
            .unwrap_err();
            assert!(error.contains(diagnostic), "{error}");
        }
    }

    #[test]
    fn javascript_and_typescript_numeric_lowering_link_without_an_execution_engine() {
        let programs = [
            crate::Engine::specialize(
                "function answer(x) { return x * 7; } answer(6);",
                "native-number.js",
            )
            .unwrap(),
            crate::Engine::specialize_typescript(
                "function answer(x: number): number { return x * 7; } answer(6);",
                "native-number.cts",
            )
            .unwrap(),
        ];
        for program in programs {
            let isa = super::super::jit_backend::target_isa("speed").unwrap();
            let native = super::super::compiled::lower_js_numeric_region(
                &program,
                1,
                &[],
                None,
                false,
                &*isa,
            )
            .expect("numeric JS residual");
            let native_signature = native.signature.clone();
            let mut main = ir::Function::with_name_signature(
                ir::UserFuncName::user(NATIVE_FUNCTION_NAMESPACE, 0),
                signature(),
            );
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut main, &mut context);
            let entry = builder.create_block();
            builder.switch_to_block(entry);
            builder.seal_block(entry);
            let signature = builder.import_signature(native_signature);
            let name = builder
                .func
                .declare_imported_user_function(ir::UserExternalName::new(
                    NATIVE_FUNCTION_NAMESPACE,
                    1,
                ));
            let callee = builder.import_function(ir::ExtFuncData {
                name: ir::ExternalName::user(name),
                signature,
                colocated: false,
                patchable: false,
            });
            let slot = builder.create_sized_stack_slot(ir::StackSlotData::new(
                ir::StackSlotKind::ExplicitSlot,
                std::mem::size_of::<[f64; 2]>() as u32,
                std::mem::align_of::<f64>().trailing_zeros() as u8,
            ));
            let arguments = builder.ins().stack_addr(isa.pointer_type(), slot, 0);
            let result = builder
                .ins()
                .iadd_imm_s(arguments, std::mem::size_of::<f64>() as i64);
            let input = builder
                .ins()
                .f64const(ir::immediates::Ieee64::with_float(6.0));
            builder
                .ins()
                .store(ir::MemFlagsData::trusted(), input, arguments, 0);
            let unused = builder.ins().iconst(isa.pointer_type(), 0);
            builder
                .ins()
                .call(callee, &[unused, unused, unused, arguments, result]);
            let output = builder
                .ins()
                .load(types::F64, ir::MemFlagsData::trusted(), result, 0);
            let status = builder.ins().fcvt_to_sint_sat(types::I32, output);
            builder.ins().return_(&[status]);
            builder.finalize(isa.frontend_config());
            let object = emit_native_object(
                "native-javascript",
                vec![
                    NativeFunction::Definition {
                        name: "main".into(),
                        function: main,
                    },
                    NativeFunction::Definition {
                        name: "answer".into(),
                        function: native,
                    },
                ],
            )
            .unwrap();
            assert_eq!(link_and_run(object), Some(PROGRAM_EXIT_STATUS as i32));
        }
    }

    #[test]
    fn imported_symbol_can_precede_its_caller() {
        let object = emit_native_object(
            "external-call",
            vec![
                NativeFunction::Import {
                    name: "getpagesize".into(),
                    signature: signature(),
                },
                function("main", Some(0)),
            ],
        )
        .unwrap();
        // POSIX exposes only the low eight bits of a normal process exit.
        const EXIT_STATUS_MASK: i32 = u8::MAX as i32;
        // SAFETY: sysconf has no pointer arguments and only queries the host.
        let expected = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as i32 & EXIT_STATUS_MASK;
        assert_eq!(link_and_run(object), Some(expected));
    }
}
