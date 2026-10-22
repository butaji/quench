from pathlib import Path
r=Path('/workspace/quench/work/prepared-realm-worktree')
def edit(n,a,b):
 p=r/n;s=p.read_text();assert a in s,n;p.write_text(s.replace(a,b))
edit('crates/quench-runtime/src/api_native.rs','    pub script_name: String,','    pub script_name: String,\n    /// True when the activation belongs to an unnamed eval Script.\n    pub is_eval: bool,')
edit('crates/quench-runtime/src/vm/embedding.rs','                script_name: if unnamed_script { String::new() } else { file_name },','                script_name: if unnamed_script { String::new() } else { file_name },\n                is_eval: unnamed_script,')
edit('crates/quench-node/src/host/shared_vm.rs','        method "utilGetCallSites"','        method "utilGetCallerLocation" (0) => crate::modules::util_shared_vm::get_caller_location,\n        method "utilGetCallSites"')
edit('crates/quench-node/src/modules/util_shared_vm.rs','        ("getCallSites", "utilGetCallSites"),','        ("getCallSites", "utilGetCallSites"),\n        ("getCallerLocation", "utilGetCallerLocation"),')
edit('crates/quench-node/src/host/shared_vm/commonjs.rs','      getCallSites: __nativeInternalBinding("util").getCallSites,','      getCallSites: __nativeInternalBinding("util").getCallSites,\n      getCallerLocation: __nativeInternalBinding("util").getCallerLocation,')
p=r/'crates/quench-node/src/modules/util_shared_vm.rs';s=p.read_text();s+='''
/// Node's native caller capture skips the immediate JavaScript activation.
pub(crate) fn get_caller_location(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, _: &[RootId],
) -> Result<RootId, RootedError> {
    let mut frames = context.source_stack_frames(2);
    let Some(frame) = (frames.len() == 2).then(|| frames.pop().unwrap()) else {
        return Ok(context.undefined());
    };
    // An unnamed eval has no ScriptNameOrSourceURL. Do not fabricate an
    // internal bootstrap caller or a script name when no such frame exists.
    if frame.is_eval { return Ok(context.undefined()); }
    let line = context.number(f64::from(frame.line_number));
    let column = context.number(f64::from(frame.column_number));
    let name = context.string_rooted(&frame.script_name);
    context.array_rooted(&[line, column, name])
}
''';p.write_text(s)
