//! `node:fs` module namespace construction: builds the main `fs` object
//! (sync + async + promises + constants) from per-op capability specs.

use super::{host_api, Value, CONSTANT_ENTRIES};

pub(super) fn build() -> Value {
    use crate::registry::*;
    let mut props: Vec<(&str, Value)> = vec![
        ("readFile", crate::host::capability(SPEC_FS_READFILE)),
        ("stat", crate::host::capability(SPEC_FS_STAT)),
        ("lstat", crate::host::capability(SPEC_FS_LSTAT)),
        ("readdir", crate::host::capability(SPEC_FS_READDIR)),
        ("exists", crate::host::capability(SPEC_FS_EXISTS)),
        ("mkdir", crate::host::capability(SPEC_FS_MKDIR)),
        ("unlink", crate::host::capability(SPEC_FS_UNLINK)),
        ("rmdir", crate::host::capability(SPEC_FS_RMDIR)),
        ("rm", crate::host::capability(SPEC_FS_RM)),
        ("rename", crate::host::capability(SPEC_FS_RENAME)),
        ("appendFile", crate::host::capability(SPEC_FS_APPENDFILE)),
        ("copyFile", crate::host::capability(SPEC_FS_COPYFILE)),
        ("access", crate::host::capability(SPEC_FS_ACCESS)),
        ("mkdtemp", crate::host::capability(SPEC_FS_MKDTEMP)),
        ("open", crate::host::capability(SPEC_FS_OPEN)),
        ("fstat", crate::host::capability(SPEC_FS_FSTAT)),
        ("close", crate::host::capability(SPEC_FS_CLOSE)),
        ("readlink", crate::host::capability(SPEC_FS_READLINK)),
        ("chmod", crate::host::capability(SPEC_FS_CHMOD)),
        ("truncate", crate::host::capability(SPEC_FS_TRUNCATE)),
        ("Stats", crate::host::capability(SPEC_FS_STATS)),
    ];
    props.extend(sync_props());
    props.push(("constants", constants()));
    props.push(("promises", promises()));
    let fs = crate::host::namespace_object(props).unwrap_or_else(|_| Value::Undefined);
    // Node exports `fs.promises` as enumerable.
    crate::host::make_property_enumerable(fs, "promises")
}

fn sync_props() -> Vec<(&'static str, Value)> {
    use crate::registry::*;
    let mut props = vec![
        (
            "readFileSync",
            crate::host::capability(SPEC_FS_READFILESYNC),
        ),
        (
            "writeFileSync",
            crate::host::capability(SPEC_FS_WRITEFILESYNC),
        ),
        ("statSync", crate::host::capability(SPEC_FS_STATSYNC)),
        ("lstatSync", crate::host::capability(SPEC_FS_LSTATSYNC)),
        ("readdirSync", crate::host::capability(SPEC_FS_READDIRSYNC)),
        ("existsSync", crate::host::capability(SPEC_FS_EXISTSSYNC)),
        ("realpathSync", crate::host::capability(SPEC_FS_REALSYNC)),
        ("mkdirSync", crate::host::capability(SPEC_FS_MKDIRSYNC)),
        ("unlinkSync", crate::host::capability(SPEC_FS_UNLINKSYNC)),
        ("rmdirSync", crate::host::capability(SPEC_FS_RMDIRSYNC)),
    ];
    props.extend(sync_props_more());
    props
}

fn sync_props_more() -> Vec<(&'static str, Value)> {
    use crate::registry::*;
    vec![
        ("openSync", crate::host::capability(SPEC_FS_OPENSYNC)),
        ("fstatSync", crate::host::capability(SPEC_FS_FSTATSYNC)),
        ("closeSync", crate::host::capability(SPEC_FS_CLOSESYNC)),
        ("rmSync", crate::host::capability(SPEC_FS_RMSYNC)),
        ("renameSync", crate::host::capability(SPEC_FS_RENAMESYNC)),
        (
            "appendFileSync",
            crate::host::capability(SPEC_FS_APPENDFILESYNC),
        ),
        (
            "copyFileSync",
            crate::host::capability(SPEC_FS_COPYFILESYNC),
        ),
        ("accessSync", crate::host::capability(SPEC_FS_ACCESSSYNC)),
        ("mkdtempSync", crate::host::capability(SPEC_FS_MKDTEMPSYNC)),
        (
            "readlinkSync",
            crate::host::capability(SPEC_FS_READLINKSYNC),
        ),
        ("chmodSync", crate::host::capability(SPEC_FS_CHMODSYNC)),
        (
            "truncateSync",
            crate::host::capability(SPEC_FS_TRUNCATESYNC),
        ),
    ]
}

/// `fs.promises` — each op runs the sync implementation and returns
/// an already-settled Promise (fulfilled with the result, rejected
/// with the coded error).
fn promises() -> Value {
    use crate::registry::*;
    let props: Vec<(&str, Value)> = vec![
        ("readFile", crate::host::capability(SPEC_FSP_READFILE)),
        ("writeFile", crate::host::capability(SPEC_FSP_WRITEFILE)),
        ("appendFile", crate::host::capability(SPEC_FSP_APPENDFILE)),
        ("stat", crate::host::capability(SPEC_FSP_STAT)),
        ("lstat", crate::host::capability(SPEC_FSP_LSTAT)),
        ("statfs", crate::host::capability(SPEC_FSP_STATFS)),
        ("readdir", crate::host::capability(SPEC_FSP_READDIR)),
        ("mkdir", crate::host::capability(SPEC_FSP_MKDIR)),
        ("unlink", crate::host::capability(SPEC_FSP_UNLINK)),
        ("rmdir", crate::host::capability(SPEC_FSP_RMDIR)),
        ("rm", crate::host::capability(SPEC_FSP_RM)),
        ("rename", crate::host::capability(SPEC_FSP_RENAME)),
        ("copyFile", crate::host::capability(SPEC_FSP_COPYFILE)),
        ("access", crate::host::capability(SPEC_FSP_ACCESS)),
        ("mkdtemp", crate::host::capability(SPEC_FSP_MKDTEMP)),
        ("readlink", crate::host::capability(SPEC_FSP_READLINK)),
        ("chmod", crate::host::capability(SPEC_FSP_CHMOD)),
        ("truncate", crate::host::capability(SPEC_FSP_TRUNCATE)),
        ("realpath", crate::host::capability(SPEC_FSP_REALPATH)),
        ("link", crate::host::capability(SPEC_FSP_LINK)),
        ("symlink", crate::host::capability(SPEC_FSP_SYMLINK)),
        ("lutimes", crate::host::capability(SPEC_FSP_LUTIMES)),
        ("lchown", crate::host::capability(SPEC_FSP_LCHOWN)),
        ("lchmod", crate::host::capability(SPEC_FSP_LCHMOD)),
        ("chown", crate::host::capability(SPEC_FSP_CHOWN)),
        ("utimes", crate::host::capability(SPEC_FSP_UTIMES)),
        ("open", crate::host::capability(SPEC_FSP_OPEN)),
    ];
    crate::host::namespace_object(props).unwrap_or_else(|_| Value::Undefined)
}

fn constants() -> Value {
    let entries: Vec<(String, Value)> = CONSTANT_ENTRIES
        .iter()
        .map(|(name, value)| (name.to_string(), Value::Number(*value)))
        .collect();
    host_api::object(entries)
}
