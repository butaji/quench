//! `process` capability properties — split into smaller groups
//! to keep the parent module's function-length lint budget under the
//! AGENTS.md 40-line ceiling.

use crate::host::capability;
use crate::registry::*;
use quench_runtime::value::Value;

pub(super) fn method_props() -> Vec<(&'static str, Value)> {
    let mut props = vec![
        ("cwd", capability(SPEC_PROCESS_CWD)),
        ("chdir", capability(SPEC_PROCESS_CHDIR)),
        ("exit", capability(SPEC_PROCESS_EXIT)),
        ("kill", capability(SPEC_PROCESS_KILL)),
        ("nextTick", capability(SPEC_PROCESS_NEXT_TICK)),
        ("hrtime", capability(SPEC_PROCESS_HRTIME)),
        ("umask", capability(SPEC_PROCESS_UMASK)),
    ];
    props.extend(event_props());
    props.extend(identity_props());
    props
}

fn event_props() -> Vec<(&'static str, Value)> {
    vec![
        ("on", capability(SPEC_PROCESS_ON)),
        ("once", capability(SPEC_PROCESS_ONCE)),
    ]
}

fn identity_props() -> Vec<(&'static str, Value)> {
    vec![
        ("getuid", capability(SPEC_PROCESS_GETUID)),
        ("getgid", capability(SPEC_PROCESS_GETGID)),
        ("binding", capability(SPEC_PROCESS_BINDING)),
    ]
}
