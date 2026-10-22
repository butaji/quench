from pathlib import Path
r=Path('/workspace/quench/work/prepared-realm-worktree')
def edit(n,a,b):
 p=r/n;s=p.read_text();assert a in s,n;p.write_text(s.replace(a,b))
edit('crates/quench-node/src/host/shared_vm.rs','        method "errorStackFormat" (3)', '        method "errorSourcePositions" (0) => crate::modules::node_errors_shared_vm::source_positions,\n        method "errorStackFormat" (3)')
edit('crates/quench-node/src/host/shared_vm/commonjs.rs','    case "fs_event_wrap": return __nativeInternalBinding(name);','    case "fs_event_wrap":\n    case "errors": return __nativeInternalBinding(name);')
edit('crates/quench-node/src/modules/trace_shared_vm.rs','        "types" => return super::util_shared_vm::types_binding(context),','''        "errors" => {
            let function = context.host_function(crate::host::shared_vm::operation("errorSourcePositions"))?;
            put(context, binding, "getErrorSourcePositions", function)?;
        }
        "types" => return super::util_shared_vm::types_binding(context),''')
p=r/'crates/quench-node/src/modules/node_errors_shared_vm.rs';s=p.read_text();s+='''
/// Project compiler-owned error source metadata for the original Node SDK policy.
pub(crate) fn source_positions(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let error = args.first().copied().unwrap_or(absent);
    if !context.is_object_rooted(error)? {
        let error = context.type_error_rooted("Error source positions require an object")?;
        return Err(context.throw(error));
    }
    let Some(position) = context.error_source_position_rooted(error)? else {
        return Ok(context.undefined());
    };
    let result = context.null_object_rooted()?;
    let source_line = position.source_line.as_deref().map_or_else(
        || context.string_rooted(""), |source| context.string_rooted(source));
    super::event_constructor_shared_vm::set(context, result, "sourceLine", source_line)?;
    let resource = match position.script_name {
        Some(name) => context.string_rooted(&name),
        None => context.undefined(),
    };
    super::event_constructor_shared_vm::set(context, result, "scriptResourceName", resource)?;
    let line = context.number(f64::from(position.line_number));
    super::event_constructor_shared_vm::set(context, result, "lineNumber", line)?;
    let column = context.number(f64::from(position.column_number.saturating_sub(1)));
    super::event_constructor_shared_vm::set(context, result, "startColumn", column)?;
    Ok(result)
}
''';s=s.replace('''    let source_line = position.source_line.as_deref().map_or_else(
        || context.string_rooted(""), |source| context.string_rooted(source));''','''    let source_line = context.string_rooted(position.source_line.as_deref().unwrap_or(""));''');p.write_text(s)
