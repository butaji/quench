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
