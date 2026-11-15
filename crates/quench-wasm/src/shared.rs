//! Decode validated modules for the shared VM. There is no executor here.

use crate::{Error, Module};

use quench_runtime::{
    Engine, WasmData, WasmDataMode, WasmFunction, WasmFunctionBody, WasmGlobal,
    WasmGlobalInitializer, WasmI32Function, WasmMemory, WasmModule, WasmType, WasmTypes,
};
use wasmparser::{Encoding, ExternalKind, Parser, Payload, ValType};

/// Defined functions from which the interpreter lowers a module's functions on their first
/// calls; a smaller module lowers at once, as compiled code always does.
const LAZY_LOWERING_FUNCTIONS: usize = 64;

mod sections;
use sections::Sections;

impl Module<'_> {
    /// Lower i32 module functions with one selected exported entry into the
    /// JavaScript VM's residual bytecode. Stateful modules and start functions
    /// require instantiation; unsupported operators fail explicitly.
    pub fn lower_shared_i32(&self, export: &str) -> Result<WasmI32Function, Error> {
        let function = self.lower_shared(export)?;
        if function
            .signature()
            .params
            .iter()
            .any(|ty| *ty != WasmType::I32)
            || function
                .signature()
                .results
                .iter()
                .any(|ty| *ty != WasmType::I32)
        {
            return Err(Error::Unsupported(
                "function requires the typed Wasm boundary".into(),
            ));
        }
        Ok(function)
    }

    /// Lower validated module functions into the single shared VM.
    pub fn lower_shared(&self, export: &str) -> Result<WasmFunction, Error> {
        let (module, index) =
            self.lower_shared_definition(Some(export), quench_runtime::ExecutionMode::Interpreted)?;
        index
            .and_then(|index| module.function(index))
            .ok_or_else(|| Error::Unsupported(format!("unknown function export: {export}")))
    }

    /// Lower module facts without requiring a function or an export.
    pub fn lower_shared_module(&self) -> Result<WasmModule, Error> {
        self.lower_shared_module_for(quench_runtime::ExecutionMode::Interpreted)
    }

    /// Lower module facts for the execution mode that will run them.
    pub fn lower_shared_module_for(
        &self,
        mode: quench_runtime::ExecutionMode,
    ) -> Result<WasmModule, Error> {
        self.lower_shared_definition(None, mode)
            .map(|(module, _)| module)
    }

    fn lower_shared_definition(
        &self,
        export: Option<&str>,
        mode: quench_runtime::ExecutionMode,
    ) -> Result<(WasmModule, Option<u32>), Error> {
        let mut sections = Sections::default();
        for payload in Parser::new(0).parse_all(self.bytes()) {
            sections.record(payload.map_err(parse_error)?, export)?;
        }
        match sections.lazy_range(export, mode) {
            Some(range) => sections.lower_lazily(self.bytes(), range, export),
            None => sections.lower_eagerly(mode, export),
        }
    }
}

fn reference_initializer(
    ty: wasmparser::RefType,
    expression: wasmparser::ConstExpr<'_>,
    globals: &[WasmGlobal],
    types: &WasmTypes,
) -> Result<quench_runtime::WasmReferenceInitializer, Error> {
    let operators = expression
        .get_operators_reader()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(parse_error)?;
    match operators.as_slice() {
        [wasmparser::Operator::RefNull { .. }, wasmparser::Operator::End] => {
            return Ok(quench_runtime::WasmReferenceInitializer::Null)
        }
        [wasmparser::Operator::RefFunc { function_index }, wasmparser::Operator::End] => {
            return Ok(quench_runtime::WasmReferenceInitializer::Function(
                *function_index,
            ))
        }
        _ => {}
    }
    let expected = types
        .callable_value_type(ValType::Ref(ty))
        .ok_or_else(|| Error::Unsupported("unsupported Wasm reference initializer type".into()))?;
    Engine::lower_wasm_constant_expression(
        "wasm-reference-initializer",
        expected,
        globals,
        operators.into_iter().map(Ok),
    )
    .map(quench_runtime::WasmReferenceInitializer::Expression)
    .map_err(|error| Error::Unsupported(error.to_string()))
}

fn decode_elements(
    reader: wasmparser::ElementSectionReader<'_>,
    globals: &[WasmGlobal],
    tables: &[quench_runtime::WasmTable],
    types: &WasmTypes,
) -> Result<Vec<quench_runtime::WasmElement>, Error> {
    use quench_runtime::{WasmElement, WasmElementMode, WasmReferenceInitializer};
    let mut elements = Vec::new();
    for element in reader {
        let element = element.map_err(parse_error)?;
        let mode = match element.kind {
            wasmparser::ElementKind::Declared => WasmElementMode::Declared,
            wasmparser::ElementKind::Passive => WasmElementMode::Passive,
            wasmparser::ElementKind::Active {
                table_index,
                offset_expr,
            } => {
                let offset = Engine::lower_wasm_constant_expression(
                    "wasm-element-offset",
                    value_type(
                        tables
                            .get(table_index.unwrap_or(0) as usize)
                            .ok_or_else(|| Error::Unsupported("unknown element table".into()))?
                            .ty
                            .index_type(),
                    )?,
                    globals,
                    offset_expr.get_operators_reader(),
                )
                .map_err(|error| Error::Unsupported(error.to_string()))?;
                WasmElementMode::Active {
                    table: table_index.unwrap_or(0),
                    offset,
                }
            }
        };
        let (element_type, items) = match element.items {
            wasmparser::ElementItems::Functions(functions) => (
                wasmparser::RefType::FUNC,
                functions
                    .into_iter()
                    .map(|index| {
                        index
                            .map(WasmReferenceInitializer::Function)
                            .map_err(parse_error)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            wasmparser::ElementItems::Expressions(ty, expressions) => {
                let items = expressions
                    .into_iter()
                    .map(|expression| {
                        reference_initializer(ty, expression.map_err(parse_error)?, globals, types)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                (ty, items)
            }
        };
        elements.push(WasmElement {
            mode,
            element_type,
            items: items.into(),
        });
    }
    Ok(elements)
}

/// Decode a global section into `globals` under `types`, checking each initializer.
pub fn decode_globals(
    reader: wasmparser::GlobalSectionReader<'_>,
    globals: &mut Vec<WasmGlobal>,
    types: &WasmTypes,
) -> Result<(), Error> {
    for global in reader {
        let global = global.map_err(parse_error)?;
        let ty = types
            .callable_value_type(global.ty.content_type)
            .ok_or_else(|| Error::Unsupported("unsupported Wasm global type".into()))?;
        let initial = WasmGlobalInitializer::Expression(
            Engine::lower_wasm_constant_expression(
                "wasm-global-initializer",
                ty,
                globals,
                global.init_expr.get_operators_reader(),
            )
            .map_err(|error| Error::Unsupported(error.to_string()))?,
        );
        if global.ty.shared {
            return Err(Error::Unsupported("unsupported shared Wasm global".into()));
        }
        globals.push(WasmGlobal {
            initial,
            mutable: global.ty.mutable,
        });
    }
    Ok(())
}

fn value_type(ty: ValType) -> Result<WasmType, Error> {
    WasmType::from_wasm(ty).ok_or_else(|| Error::Unsupported("unsupported Wasm value type".into()))
}

fn parse_error(error: wasmparser::BinaryReaderError) -> Error {
    Error::Parse(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quench_runtime::{ExecutionRequest, Host, Runtime};

    /// The one host these tests run under: each host type compiles the VM again.
    #[derive(Default)]
    pub(super) struct TestHost;
    impl Host for TestHost {
        fn write_line(&mut self, _: &str) {}
        fn clock_millis(&mut self) -> f64 {
            0.0
        }
    }

    fn lower(wat: &str) -> WasmI32Function {
        crate::Engine::new()
            .compile_wat(wat)
            .unwrap()
            .lower_shared_i32("f")
            .unwrap()
    }

    #[test]
    fn shared_void_result_and_argument_count_are_explicit() {
        let function = lower(
            "(module (func (export \"f\") (param i32) (local i32) local.get 0 local.set 1 nop))",
        );
        let mut runtime = Runtime::new(TestHost);
        assert!(runtime.execute_wasm_i32(&function, &[]).is_err());
        assert!(runtime.execute_wasm_i32(&function, &[1, 2]).is_err());
        assert_eq!(runtime.execute_wasm_i32(&function, &[1]).unwrap(), None);
    }

    #[test]
    fn shared_wasm_and_javascript_use_the_same_runtime() {
        let mut runtime = Runtime::new(TestHost);
        let function = lower("(module (func (export \"f\") (result i32) i32.const -2147483648))");
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "if (2147483647 + 1 !== 2147483648) throw 'JS overflow';",
                "arithmetic.js",
            ))
            .unwrap();
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[]).unwrap(),
            Some(i32::MIN)
        );
        runtime
            .compile_and_execute(ExecutionRequest::script(
                "if (2 * 3 !== 6) throw 'JS multiply';",
                "arithmetic.js",
            ))
            .unwrap();
    }

    #[test]
    fn shared_boundary_rejects_wrong_typed_entry_and_missing_export() {
        let module = crate::Engine::new()
            .compile_wat("(module (func (export \"f\") (result i64) i64.const 1))")
            .unwrap();
        assert!(matches!(
            module.lower_shared_i32("f"),
            Err(Error::Unsupported(_))
        ));
        let module = crate::Engine::new().compile_wat("(module)").unwrap();
        assert!(module.lower_shared_i32("missing").is_err());
    }

    #[test]
    fn type_declarations_preserve_recursive_indices_and_shared_ownership() {
        let module = crate::Engine::new()
            .compile_wat(
                "(module
                  (rec
                    (type $a (sub (struct (field (ref null $b)))))
                    (type $b (sub (struct (field (ref null $a))))))
                  (type $derived (sub $a (struct (field (ref null $b)))))
                  (type $plain (func (result i32)))
                  (func (type $plain) (i32.const 7)))",
            )
            .unwrap();
        let lowered = module.lower_shared_module().unwrap();
        let types = lowered.types();
        assert_eq!(types.groups().len(), 3);
        assert_eq!(types.groups()[0].len(), 2);
        assert!(!types.get(0).unwrap().is_final);
        assert_eq!(
            types.get(2).unwrap().supertype_idx.unwrap().unpack(),
            wasmparser::UnpackedIndex::Module(0)
        );
        assert_eq!(
            types.get(0).unwrap().unwrap_struct().fields[0].element_type,
            wasmparser::StorageType::Val(wasmparser::ValType::Ref(
                wasmparser::RefType::new(
                    true,
                    wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(1)),
                )
                .unwrap(),
            ))
        );
        assert!(types.function("type-domain", 0).is_err());
        assert_eq!(
            types.function("type-domain", 3).unwrap().results(),
            &[ValType::I32]
        );
        assert!(types.get(4).is_none());
        let cloned = lowered.clone();
        assert!(std::ptr::eq(types.groups(), cloned.types().groups()));
        let entry = lowered.function(0).unwrap();
        assert_eq!(entry.signature().results, vec![WasmType::I32]);
    }

    #[test]
    fn recursive_type_equivalence_and_ancestry_match_validator_identity() {
        let declarations = |module: &Module<'_>| {
            let mut groups = Vec::new();
            for payload in Parser::new(0).parse_all(module.bytes()) {
                if let Payload::TypeSection(reader) = payload.unwrap() {
                    groups.extend(reader.into_iter().map(Result::unwrap));
                }
            }
            WasmTypes::from_groups(groups)
        };
        let sources = [
            "(module (type (struct (field (ref null 0)))))",
            "(module (type (func)) (type (struct (field (ref null 1)))))",
            "(module (type (struct (field (ref null 0)))) (type (struct (field (ref null 0)))))",
            "(module (rec (type (struct (field (ref null 0)))) (type (struct (field (ref null 1))))))",
            "(module (rec (type (struct (field (ref null 1)))) (type (struct (field (ref null 0))))))",
            "(module (rec (type (struct (field (ref null 1)))) (type (struct (field i32)))))",
            "(module (type (struct (field (ref 0)))))",
            "(module (type (struct (field (mut (ref null 0))))))",
            "(module (type (array (mut i8))))",
            "(module (type (array (mut i16))))",
            "(module (type (array i8)))",
            "(module (type (sub (struct))) (type (sub 0 (struct))))",
            "(module (type (func)) (type (sub (struct))) (type (sub 1 (struct))))",
            "(module (type (sub (struct))) (type (sub final 0 (struct))))",
            "(module (type (struct)))",
            "(module (type (sub (func (param i32) (result i32)))))",
            "(module (type (func (param i32) (result i32))))",
            "(module (type (func (param i64) (result i32))))",
            "(module (type (struct)) (type (func (param (ref null 0)))))",
            "(module (type (func)) (type (struct)) (type (func (param (ref null 1)))))",
        ];
        let mut validator = wasmparser::Validator::new_with_features(crate::core_features());
        let mut samples = Vec::new();
        for source in sources {
            let module = crate::Engine::new().compile_wat(source).unwrap();
            let declarations = declarations(&module);
            validator.reset();
            let validated = validator.validate_all(module.bytes()).unwrap();
            let validated = validated.as_ref();
            let count = declarations.groups().iter().map(Vec::len).sum::<usize>();
            let ids: Vec<_> = (0..count)
                .map(|index| validated.core_type_at_in_module(index as u32))
                .collect();
            let ancestors: Vec<Vec<_>> = ids
                .iter()
                .map(|&id| {
                    let mut chain = vec![id];
                    let mut next = validated.supertype_of(id);
                    while let Some(parent) = next {
                        chain.push(parent);
                        next = validated.supertype_of(parent);
                    }
                    chain
                })
                .collect();
            samples.push((declarations, ids, ancestors));
        }
        for (left, left_ids, ancestors) in &samples {
            for (right, right_ids, _) in &samples {
                for (left_index, left_id) in left_ids.iter().enumerate() {
                    for (right_index, right_id) in right_ids.iter().enumerate() {
                        assert_eq!(
                            left.equivalent(left_index, right, right_index),
                            left_id == right_id,
                            "canonical group/member identity: {left_index}, {right_index}"
                        );
                        assert_eq!(
                            left.is_subtype(left_index, right, right_index),
                            ancestors[left_index].contains(right_id),
                            "declared ancestry: {left_index}, {right_index}"
                        );
                    }
                }
            }
            assert!(!left.equivalent(left_ids.len(), left, 0));
            assert!(!left.is_subtype(0, left, left_ids.len()));
        }
        const TYPE_CHAIN_DEPTH: usize = 1024;
        let mut deep = Vec::new();
        for (prefix, shift) in [("", 0), ("(type (func))", 1)] {
            let mut source = format!("(module {prefix} (type (struct (field i32)))");
            for index in 1..TYPE_CHAIN_DEPTH {
                let parent = index - 1 + shift;
                source.push_str(&format!(
                    "(type (struct (field (ref null {parent})) (field (ref null {parent}))))"
                ));
            }
            source.push(')');
            let module = crate::Engine::new().compile_wat(&source).unwrap();
            validator.reset();
            let validated = validator.validate_all(module.bytes()).unwrap();
            let index = TYPE_CHAIN_DEPTH - 1 + shift;
            deep.push((
                declarations(&module),
                index,
                validated.as_ref().core_type_at_in_module(index as u32),
            ));
        }
        assert_eq!(deep[0].2, deep[1].2);
        assert!(deep[0].0.equivalent(deep[0].1, &deep[1].0, deep[1].1));
        assert!(deep[0].0.is_subtype(deep[0].1, &deep[1].0, deep[1].1));
        let count = samples.iter().map(|(_, ids, _)| ids.len()).sum::<usize>();
        println!(
            "canonical identity/ancestry oracle: {} type-pair checks; depth={TYPE_CHAIN_DEPTH}",
            count * count * 2
        );
    }

    #[test]
    fn shared_lowering_uses_wide_encoding_for_deep_stacks() {
        let wat = format!(
            "(module (func (export \"f\") (result i32) {} {}))",
            "i32.const 1 ".repeat(300),
            "i32.add ".repeat(299)
        );
        let function = lower(&wat);
        let mut runtime = Runtime::new(TestHost);
        assert_eq!(runtime.execute_wasm_i32(&function, &[]).unwrap(), Some(300));
    }

    #[test]
    fn shared_integer_traps_are_typed_and_runtime_recovers() {
        use quench_runtime::WasmTrap;

        let divide = lower(
            "(module (func (export \"f\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.div_s))",
        );
        let remainder = lower(
            "(module (func (export \"f\") (param i32 i32) (result i32) local.get 0 local.get 1 i32.rem_s))",
        );
        let mut runtime = Runtime::new(TestHost);
        for (args, trap) in [
            ([i32::MIN, 0], WasmTrap::IntegerDivideByZero),
            ([i32::MIN, -1], WasmTrap::IntegerOverflow),
        ] {
            let error = runtime.execute_wasm_i32(&divide, &args).unwrap_err();
            assert_eq!(error.wasm_trap(), Some(trap));
            assert_eq!(error.to_string(), trap.to_string());
            runtime.collect(divide.residual()).unwrap();
            assert_eq!(runtime.execute_wasm_i32(&divide, &[7, 2]).unwrap(), Some(3));
        }
        assert_eq!(
            runtime
                .execute_wasm_i32(&remainder, &[i32::MIN, -1])
                .unwrap(),
            Some(0)
        );
        let error = runtime.execute_wasm_i32(&divide, &[]).unwrap_err();
        assert_eq!(error.wasm_trap(), None);

        let program =
            quench_runtime::Engine::specialize("throw new Error('guest');", "throw.js").unwrap();
        assert_eq!(runtime.execute(&program).unwrap_err().wasm_trap(), None);
    }
}

#[cfg(test)] mod control_tests;
#[cfg(test)] mod call_tests;
#[cfg(test)] mod scalar_tests;
#[cfg(test)] mod integer_tests;
#[cfg(test)] mod float_tests;
#[cfg(test)] mod export_call_tests;
