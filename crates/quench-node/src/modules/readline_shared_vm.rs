//! Line-oriented input adapter for Node's `readline` CommonJS builtin.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const FACTORY: &str = quench_js_check::checked_js!(
    r#"(EventEmitter) => {
  class Interface extends EventEmitter {
    constructor(options) {
      super();
      if (!options || !options.input || typeof options.input.on !== "function") {
        throw new TypeError("The \"input\" argument must be an instance of Readable");
      }
      this.input = options.input;
      this.closed = false;
      this._lineBuffer = "";
      this._crlfDelay = options.crlfDelay === undefined ? 100 : options.crlfDelay;
      this._onData = (chunk) => this._write(chunk);
      this._onEnd = () => this._close();
      this._onError = (error) => this.emit("error", error);
      this.input.on("data", this._onData);
      this.input.on("end", this._onEnd);
      this.input.on("error", this._onError);
    }
    _write(chunk) {
      if (this.closed) return;
      this._lineBuffer += typeof chunk === "string" ? chunk : String(chunk);
      let newline;
      while ((newline = this._lineBuffer.indexOf("\n")) !== -1) {
        let line = this._lineBuffer.slice(0, newline);
        if (line.endsWith("\r")) line = line.slice(0, -1);
        this._lineBuffer = this._lineBuffer.slice(newline + 1);
        this.emit("line", line);
      }
    }
    _close() {
      if (this.closed) return;
      this.closed = true;
      if (this._lineBuffer.length) {
        const line = this._lineBuffer.endsWith("\r")
          ? this._lineBuffer.slice(0, -1)
          : this._lineBuffer;
        this._lineBuffer = "";
        this.emit("line", line);
      }
      this.emit("close");
    }
    close() { this.input.pause?.(); this._close(); }
    pause() { this.input.pause?.(); return this; }
    resume() { this.input.resume?.(); return this; }
    prompt() {}
    setPrompt() {}
    write(data) { this._write(data); }
  }
  function createInterface(input, output, completer, terminal) {
    const options = input && typeof input === "object" && "input" in input
      ? input
      : { input, output, completer, terminal };
    return new Interface(options);
  }
  return { Interface, Readline: Interface, createInterface };
}"#
);

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let events = crate::modules::events_shared_vm::module(context)?;
    let event_emitter = get(context, events, "EventEmitter")?;
    let factory = context.evaluate_script_rooted(FACTORY, "node:readline/shared.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[event_emitter])
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let name = context.string_rooted(name);
    context.get_property_rooted(object, name)
}
