//! The callee a failed call names in its TypeError, as V8's `CallPrinter` renders it
//! ("o.a.b is not a function", "(0 , f) is not a function"): the call at the throwing
//! position is found in source on the dynamic error path. Native images freeze the same
//! descriptions before publication, so their error path needs no syntax frontend.

use super::Vm;
use crate::bytecode::{ResidualProgram, WideInstruction};
use crate::heap::Cell;
use crate::value::Value;

/// The message templates (V8 `kCalledNonCallable`, `kNotConstructor`).
const NOT_A_FUNCTION: &str = " is not a function";
const NOT_A_CONSTRUCTOR: &str = " is not a constructor";

/// The messages the call paths give a non-callable callee before its call site is known.
pub const NOT_CALLABLE_MESSAGE: &str = "value is not callable";
pub const NOT_CONSTRUCTOR_MESSAGE: &str = "value is not a constructor";

/// How a call instruction names its callee.
enum Callee {
    /// The callee is this value.
    Value(Value),
    /// The callee is the receiver's `atom` property, read without running guest code.
    Method { receiver: Value, atom: crate::bytecode::Atom },
}

impl Vm {
    /// `error` with the callee named when it is the call paths' failure to call or construct
    /// the callee of the call at `pc` (`NOT_CALLABLE_MESSAGE`, `NOT_CONSTRUCTOR_MESSAGE`):
    /// the TypeError V8 throws there. Any other error is returned as it is.
    pub(super) fn name_non_callable_callee(&mut self, program: &ResidualProgram, frame: usize, function: usize, pc: u32, error: super::JsError) -> super::JsError {
        let construct = if error.has_message(NOT_CALLABLE_MESSAGE) {
            false
        } else if error.has_message(NOT_CONSTRUCTOR_MESSAGE) {
            true
        } else {
            return error;
        };
        match self.non_callable_call_site_message(program, frame, function, pc, construct) {
            Some(message) => self.type_error(program, message),
            None => error,
        }
    }

    /// The message for calling (or, with `construct`, constructing) a non-callable callee at
    /// `pc` of `function`, the instruction `frame` runs: `None` when that instruction is not a
    /// call whose own callee is not callable, so the failure is from inside the call.
    pub(super) fn non_callable_call_site_message(&self, program: &ResidualProgram, frame: usize, function: usize, pc: u32, construct: bool) -> Option<String> {
        let instruction = program.function(function).instructions().nth(pc as usize)?;
        let callee = self.call_instruction_callee(program, frame, instruction, construct)?;
        let value = match callee {
            Callee::Value(value) => Some(value),
            Callee::Method { receiver, atom } => self.data_property_value_without_getters(receiver, atom),
        };
        // An accessor or proxy hides the callee: the failure may be anywhere, so keep the
        // generic message.
        let value = value?;
        let callable = if construct { self.is_constructable(value) } else { self.is_function(value) };
        if callable {
            return None;
        }
        let template = if construct { NOT_A_CONSTRUCTOR } else { NOT_A_FUNCTION };
        let rendered = program
            .source_offset(function as u32, pc)
            .and_then(|offset| {
                let source = program.source.as_ref()?;
                let span = program.functions[function].header().source_span;
                call_site_callee(source, span, (offset, construct))
            })
            .unwrap_or_else(|| self.default_call_site(value));
        Some(rendered + template)
    }

    fn call_instruction_callee(&self, program: &ResidualProgram, frame: usize, instruction: WideInstruction, construct: bool) -> Option<Callee> {
        let register = |index: u16| self.frames.get(frame)?.registers.get(usize::from(index)).copied();
        if call_kind(instruction)? != construct { return None; }
        Some(if crate::staged::calls_method_site(instruction.op()) {
            Callee::Method {
                receiver: register(instruction.register_b())?,
                atom: program.method_sites.get(instruction.method_site_index())?.atom,
            }
        } else {
            Callee::Value(register(instruction.register_b())?)
        })
    }

    /// `receiver[atom]` when every object on its prototype chain answers it with a data
    /// property or nothing (undefined); `None` when an accessor or proxy would run code.
    fn data_property_value_without_getters(&self, receiver: Value, atom: crate::bytecode::Atom) -> Option<Value> {
        let key = crate::vm::PropertyKey::string(atom);
        let mut holder = self.primitive_prototype(receiver).unwrap_or(receiver);
        loop {
            if holder.is_null() || holder.is_undefined() {
                return Some(Value::UNDEFINED);
            }
            if matches!(self.heap.get(holder), Some(Cell::Proxy { .. })) {
                return None;
            }
            if let Some(attributes) = self.property_attributes(holder, key) {
                return (!attributes.accessor).then(|| self.own_property(holder, atom)).flatten();
            }
            holder = match self.heap.object(holder) {
                Some(object) => object.proto(),
                None => return Some(Value::UNDEFINED),
            };
        }
    }

    /// V8's `BuildDefaultCallSite`: the callee's type and, for a primitive, its value.
    fn default_call_site(&self, value: Value) -> String {
        let kind = super::coercion::typeof_text(self.typeof_kind(value));
        if let Some(text) = self.heap.string(value) {
            return format!("{kind} \"{text}\"");
        }
        if value.is_null() {
            return format!("{kind} null");
        }
        if let Some(flag) = value.as_bool() {
            return format!("{kind} {flag}");
        }
        if let Some(number) = value.as_number() {
            return format!("{kind} {}", crate::number_to_string::format(number));
        }
        kind.to_owned()
    }
}

fn call_kind(instruction: WideInstruction) -> Option<bool> {
    // merge: HEAD's named_callee_is_construction is replaced by the derivation's construction and makes_call tests.
    if crate::staged::Operation::<u64, ()>::is_construction(instruction)
        && !matches!(crate::staged::Operation::<u64, ()>::of(instruction), Some(crate::staged::Operation::SuperCall { .. }))
    {
        Some(true)
    } else if crate::staged::makes_call(instruction.op()) {
        Some(false)
    } else {
        None
    }
}

/// Freeze source-derived descriptions without opening or compiling lazy function bodies.
/// A native-only build freezes them as it compiles; a dynamic one on their first read
/// (`frozen_call_sites`) or before writing the program, so a load that raises no error
/// parses its source once.
pub fn freeze_call_sites(program: &ResidualProgram) {
    if let Some(source) = &program.source {
        freeze_source_call_sites(source);
    }
}

/// The call-site descriptions of `source`: frozen ones, or, in a dynamic build, those its
/// text gives now. Native-only execution never reconstructs syntax on a diagnostic path.
fn frozen_call_sites(source: &crate::bytecode::ProgramSource) -> Option<&[crate::bytecode::CallSiteDescription]> {
    if crate::DYNAMIC_EXECUTION_ENABLED && let Some(routes) = crate::compile::source_routes::routes() {
        (routes.freeze_call_sites)(source);
    }
    source.call_sites.get().map(|sites| &sites[..])
}

/// Freezes `source`'s call sites from its text, once (`compile::call_sites`).
pub fn freeze_source_call_sites(source: &crate::bytecode::ProgramSource) {
    source.call_sites.get_or_init(|| crate::compile::call_sites::source_call_sites(source).into());
}

/// The admitted source expression at an exact call position. Native-only execution
/// consumes frozen facts; it never reconstructs syntax on a diagnostic path.
pub(super) fn source_expression(program: &ResidualProgram, line: u32, column: u32) -> Option<String> {
    let source = program.source.as_ref()?;
    let offset = source.byte_offset(crate::bytecode::SourcePosition { line, column })?;
    let sites = frozen_call_sites(source)?;
    let index = sites.binary_search_by_key(&offset, |site| site.offset).ok()?;
    let span = sites[index].expression?;
    source.text.get(span.start as usize..span.end as usize).map(str::to_owned)
}

/// The callee text of the call at `site` of a function whose text is `span`. Before the
/// source's call sites are frozen, a dynamic build reads them from the function's text alone
/// (`compile::call_sites::unfrozen_call_sites`), so an error a load raises and catches costs
/// its function, not its whole source.
fn call_site_callee(source: &crate::bytecode::ProgramSource, span: Option<crate::bytecode::SourceSpan>, site: (u32, bool)) -> Option<String> {
    let lookup = |sites: &[crate::bytecode::CallSiteDescription]| {
        sites.binary_search_by_key(&site, |found| (found.offset, found.construct)).ok().map(|index| sites[index].callee.clone())
    };
    if crate::DYNAMIC_EXECUTION_ENABLED && source.call_sites.get().is_none()
        && let Some(routes) = crate::compile::source_routes::routes()
        && let Some(callee) = (routes.unfrozen_call_sites)(source, span, site.0).and_then(|sites| lookup(&sites))
    {
        return Some(callee);
    }
    lookup(frozen_call_sites(source)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn assertion_expression_projection_follows_node_token_boundaries() {
        for (text, position, expected) in [
            ("assert(false)", "assert", "assert(false)"),
            ("assert.ok(false)", "ok(", "assert.ok(false)"),
            ("assert['ok'](false)", "(false)", "assert['ok'](false)"),
            ("assert?.ok(false)", "ok(", "assert?.ok(false)"),
            ("(0, assert)(false)", "(false)", "(false)"),
            ("assert[(() => 'ok')()](false)", "(false)", "(false)"),
            ("assert.ok(\n  false\n)", "ok(", "assert.ok("),
            ("const text='😀'; assert.ok(text === '')", "ok(", "assert.ok(text === '')"),
            ("new assert(false)", "new assert", "new assert(false)"),
            ("assert.ok(/[(;)]/.test(''))", "ok(", "assert.ok(/[(;)]/.test(''))"),
        ] {
            let program = crate::Engine::specialize(text, "assertion-source.js").unwrap();
            freeze_call_sites(&program);
            let source = program.source.as_ref().unwrap();
            let site = source.call_sites.get().expect("facts are frozen from source").iter()
                .find(|site| site.offset as usize == text.find(position).unwrap())
                .expect("assertion call");
            let actual = site.expression.map(|span| &source.text[span.start as usize..span.end as usize]);
            assert_eq!(actual, Some(expected), "{text}");
        }
        let program = crate::Engine::specialize("assert.ok((()=>{return false;})())", "semicolon-expression.js").unwrap();
        freeze_call_sites(&program);
        let sites = program.source.as_ref().unwrap().call_sites.get().unwrap();
        assert!(sites.iter().find(|site| site.callee == "assert.ok").unwrap().expression.is_none(),
            "Node's semicolon terminator yields an empty expression");
    }

    #[test]
    fn diagnostics_keep_lazy_records_and_native_lookup_never_parses() {
        let mut program = crate::Engine::specialize(
            "function cold(value) { assert.ok(value); }", "cold-diagnostics.js").unwrap();
        let released: Vec<_> = program.functions.iter().map(crate::bytecode::FunctionSlot::is_released).collect();
        assert!(released.iter().any(|released| *released), "the fixture must keep a cold record");
        // A dynamic build defers the facts to their first read; a native-only one admits them.
        assert_eq!(program.source.as_ref().unwrap().call_sites.get().is_some(), !crate::DYNAMIC_EXECUTION_ENABLED);
        program.source.as_mut().unwrap().call_sites.take();
        freeze_call_sites(&program);
        assert_eq!(program.functions.iter().map(crate::bytecode::FunctionSlot::is_released).collect::<Vec<_>>(), released);
        let source = program.source.as_ref().unwrap();
        let sites = source.call_sites.get().unwrap();
        let owner = sites.as_ptr();
        let column = "function cold(value) { assert.".len() as u32 + 1;
        for _ in 0..2 {
            assert_eq!(source_expression(&program, 1, column).as_deref(), Some("assert.ok(value)"));
            assert_eq!(program.source.as_ref().unwrap().call_sites.get().unwrap().as_ptr(), owner);
        }
        program.source.as_mut().unwrap().call_sites.take();
        if crate::DYNAMIC_EXECUTION_ENABLED {
            assert_eq!(source_expression(&program, 1, column).as_deref(), Some("assert.ok(value)"), "a dynamic lookup derives them");
        } else {
            assert_eq!(source_expression(&program, 1, column), None, "lookup never derives missing facts");
            assert!(program.source.as_ref().unwrap().call_sites.get().is_none());
        }
    }

    #[test]
    fn frozen_expression_ranges_are_validated_by_binary_and_snapshot_readers() {
        use crate::bytecode::SourceSpan;
        let text = "const face='😀'; call();\nnext();";
        let offset = text.find("call").unwrap() as u32;
        let emoji = text.find('😀').unwrap() as u32;
        for span in [
            SourceSpan { start: offset + 1, end: offset + 6 },
            SourceSpan { start: 0, end: offset - 1 },
            SourceSpan { start: offset + 6, end: offset },
            SourceSpan { start: emoji + 1, end: offset + 6 },
            SourceSpan { start: offset, end: u32::MAX },
            SourceSpan { start: offset, end: text.len() as u32 },
        ] {
            let mut program = crate::Engine::specialize(text, "invalid-expression.js").unwrap();
            program.freeze_native_diagnostics();
            let source = program.source.as_mut().unwrap();
            let mut sites = source.call_sites.take().unwrap().to_vec();
            sites.iter_mut().find(|site| site.offset == offset).unwrap().expression = Some(span);
            source.call_sites = std::cell::OnceCell::from(std::sync::Arc::from(sites));
            let mut writer = crate::snapshot::Writer::new();
            source.write_snapshot(&mut writer);
            let bytes = writer.finish();
            let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
            assert!(crate::bytecode::ProgramSource::read_snapshot(&mut reader).is_err(), "{span:?}");
            let decoded = crate::ResidualProgram::decode_trusted(std::rc::Rc::new(program.encode_trusted())).unwrap();
            assert!(decoded.validate().unwrap_err().contains("call-site expression"), "{span:?}");
        }
    }

    #[test]
    fn frozen_descriptions_cover_every_call_and_survive_both_encodings() {
        let body = "const marker = '😀'; function test() { missing(); new Missing(); object.deep.method(); (0, object.method)(); object['key'](); optional?.(); }";
        for (text, typescript, retained) in [
            (body.to_owned(), false, None),
            (format!("const typed: number = 1; {body}"), true, None),
            (format!("(function () {{\n  {body}\n}})"), false, Some(("(function () {\n  ", body))),
        ] {
            let origin = retained.map(|(prefix, _)| crate::wrapper_origin(prefix).unwrap());
            let mut program = crate::Engine::specialize_host_script(&text, "calls.js", &crate::compile::NoAtomPrefix,
                crate::HostScriptOptions { origin, typescript, keep_records: true }, crate::compile::ReleasedRecords::Encoded, false).unwrap();
            if let Some((prefix, body)) = retained {
                let source = program.source.as_mut().unwrap();
                source.retain_body(crate::bytecode::SourceText::Shared(body.into()), prefix.len() as u32);
                (source.line_offset, source.column_offset) = origin.unwrap();
            }
            let source = program.source.as_ref().unwrap();
            let mut calls = std::collections::BTreeSet::new();
            for function in 0..program.function_count() {
                for (pc, instruction) in program.function(function).instructions().enumerate() {
                    if let Some(construct) = call_kind(instruction)
                        && let Some(offset) = program.source_offset(function as u32, pc as u32)
                    { calls.insert((offset, construct)); }
                }
            }
            program.freeze_native_diagnostics();
            let actual: BTreeMap<_, _> = source.call_sites.get().unwrap().iter()
                .map(|site| ((site.offset, site.construct), site.callee.clone())).collect();
            assert!(!calls.is_empty(), "typescript={typescript} retained={retained:?}");
            assert!(calls.iter().all(|call| actual.contains_key(call)), "typescript={typescript} retained={retained:?}");
            for callee in ["missing", "Missing", "object.deep.method"] {
                assert!(actual.values().any(|rendered| rendered == callee), "{callee}: typescript={typescript} retained={retained:?}");
            }
            let decoded = crate::ResidualProgram::decode_trusted(std::rc::Rc::new(program.encode_trusted())).unwrap();
            assert_eq!(decoded.source.as_ref().unwrap().call_sites.get(), source.call_sites.get());
            let mut writer = crate::snapshot::Writer::new();
            source.write_snapshot(&mut writer);
            let bytes = writer.finish();
            let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
            let restored = crate::bytecode::ProgramSource::read_snapshot(&mut reader).unwrap();
            reader.finish().unwrap();
            assert_eq!(restored.call_sites.get(), source.call_sites.get());
        }
    }

    #[test]
    fn invalid_frozen_descriptions_are_rejected_and_empty_facts_are_complete() {
        let program = crate::Engine::specialize("42;", "empty.js").unwrap();
        program.freeze_native_diagnostics();
        assert!(program.source.as_ref().unwrap().call_sites.get().unwrap().is_empty());
        for sites in [
            vec![(0, "first"), (0, "duplicate")],
            vec![(2, "later"), (0, "earlier")],
            vec![(u32::MAX, "outside")],
            vec![(0, "")],
        ] {
            let mut invalid = crate::Engine::specialize("42;", "invalid.js").unwrap();
            invalid.source.as_mut().unwrap().call_sites = std::cell::OnceCell::from(std::sync::Arc::from(sites.into_iter()
                .map(|(offset, callee)| crate::bytecode::CallSiteDescription { offset, construct: false, callee: callee.into(), expression: None })
                .collect::<Vec<_>>()));
            let decoded = crate::ResidualProgram::decode_trusted(std::rc::Rc::new(invalid.encode_trusted())).unwrap();
            assert!(decoded.validate().unwrap_err().contains("frozen call-site"));
        }
    }

    #[test]
    fn typescript_assertions_and_type_arguments_preserve_callee_identity() {
        let program = crate::Engine::specialize_typescript(
            "const f = 0 as unknown as {(): unknown; new(): unknown}; (f as any)(); f!(); f?.<() => void>(); new (f as any)();",
            "typed-calls.cts",
        ).unwrap();
        program.freeze_native_diagnostics();
        let sites = program.source.as_ref().unwrap().call_sites.get().unwrap();
        assert_eq!(sites.len(), 4);
        assert!(sites.iter().all(|site| site.callee == "f"), "{sites:?}");
    }
}
