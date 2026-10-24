use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, Value};

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let stream =
        crate::host::shared_vm::commonjs::node_library_module(context, "stream.js", "stream.js")?;
    let readline = crate::host::shared_vm::commonjs::node_library_module(
        context,
        "readline.js",
        "readline.js",
    )?;
    let duplex_key = context.string_rooted("Duplex");
    let duplex = context.get_property_rooted(stream, duplex_key)?;
    let isatty = context.host_function(crate::host::shared_vm::operation("ttyIsatty"))?;
    let factory = context.evaluate_script_rooted(
        r#"(Duplex, readline, isatty) => {
          'use strict';
          class ReadStream extends Duplex {
            constructor(fd, options = {}) {
              if (!Number.isInteger(fd) || fd < 0) throw new RangeError('Invalid file descriptor');
              super({ readableHighWaterMark: 0, ...options });
              this.fd = fd;
              this.isTTY = true;
              this.isRaw = false;
              this.rawMode = false;
            }
            setRawMode(mode) { this.isRaw = Boolean(mode); this.rawMode = this.isRaw; return this; }
          }
          class WriteStream extends Duplex {
            constructor(fd, options = {}) {
              if (!Number.isInteger(fd) || fd < 0) throw new RangeError('Invalid file descriptor');
              super({ readableHighWaterMark: 0, ...options });
              this.fd = fd;
              this.isTTY = true;
              this.columns = 0;
              this.rows = 0;
            }
            cursorTo(...args) { return readline.cursorTo(this, ...args); }
            moveCursor(...args) { return readline.moveCursor(this, ...args); }
            clearLine(...args) { return readline.clearLine(this, ...args); }
            clearScreenDown(...args) { return readline.clearScreenDown(this, ...args); }
            getColorDepth() { return 1; }
            hasColors(count = 16) { return count <= 2; }
          }
          WriteStream.prototype.isTTY = true;
          return { ReadStream, WriteStream, isatty };
        }"#,
        "node:tty/facade.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[duplex, readline, isatty])
}

pub(crate) fn isatty(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let terminal = args
        .first()
        .and_then(|root| context.rooted_value(*root))
        .and_then(Value::as_number)
        .filter(|fd| {
            fd.is_finite() && fd.fract() == 0.0 && (i32::MIN as f64..=i32::MAX as f64).contains(fd)
        })
        .is_some_and(|fd| crate::modules::tty_state::is_terminal_fd(fd as i32));
    Ok(context.boolean(terminal))
}
