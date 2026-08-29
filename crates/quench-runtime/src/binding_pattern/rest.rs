fn assign_rest_target(
    target: &AssignmentTarget<'_>,
    iterator: u16,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    if let Some(place) =
        crate::reduce::reduce_assignments::reduce_place(target, ops, facts, next, locals)
    {
        let value = iterator_rest(iterator, ops, next);
        return crate::reduce::reduce_assignments::put(place, value, ops);
    }
    let value = iterator_rest(iterator, ops, next);
    assign_target(target, value, ops, facts, next, locals)
}
