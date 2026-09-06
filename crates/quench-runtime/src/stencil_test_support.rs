pub(crate) fn visit_code_views(
    view: crate::machine::CodeView<'_>,
    visit: &mut impl FnMut(crate::machine::CodeView<'_>),
) {
    visit(view);
    view.cold_ops().for_each(|(_, op)| {
        op.visit_bodies(&mut |body| {
            if let Some(nested) = body.code() {
                visit_code_views(nested, visit);
            }
        });
    });
}

pub(crate) fn cyclic_function_root() -> (
    crate::value::Value,
    std::rc::Weak<crate::value::FunctionValue>,
) {
    use std::{cell::RefCell, rc::Rc};

    let captures = crate::environment::Environment::new();
    let function = Rc::new(crate::value::FunctionValue {
        code: crate::machine::FunctionCode::from_ops(vec![crate::ops::Op::Return { src: 0 }]),
        params: 0,
        captures: Rc::clone(&captures),
        with_captures: Vec::new(),
        properties: Rc::new(RefCell::new(Vec::new())),
        private_slots: Rc::new(RefCell::new(Vec::new())),
        private_environment: Default::default(),
        instance_fields: Rc::new(RefCell::new(Vec::new())),
        kind: crate::ops::FunctionKind::Ordinary,
        strictness: crate::ops::FunctionStrictness::Sloppy,
        is_async: false,
        mapped_arguments: false,
    });
    crate::cycle_collector::track_function(&function);
    captures.set(0, crate::value::Value::Function(Rc::clone(&function)));
    let weak = Rc::downgrade(&function);
    (crate::value::Value::Function(function), weak)
}
