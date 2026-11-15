use super::*;

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
            crate::HostScriptOptions { origin, typescript, keep_records: true }, crate::compile::ReleasedRecords::Encoded).unwrap();
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
                    && let Some(offset) = program.source_position(function as u32, pc as u32).and_then(|position| source.byte_offset(position))
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
