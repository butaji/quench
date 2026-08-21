//! Polyfill: `tty`

pub const JS: &str = quench_js_check::checked_js!(r#"const __quenchOriginalRequireWithTty = globalThis.require;
class __quenchWriteStream {
  constructor(fd) {
    this.fd = fd;
    this.isTTY = false;
    this.columns = undefined;
    this.rows = undefined;
  }
  getColorDepth() {
    return 1;
  }
  hasColors() {
    return false;
  }
  getWindowSize() {
    return [this.columns || 0, this.rows || 0];
  }
}
class __quenchReadStream extends __quenchWriteStream {}
const __quenchTtyModule = {
  isatty: () => false,
  ReadStream: __quenchReadStream,
  WriteStream: __quenchWriteStream,
};
globalThis.require = (specifier) => {
  if (String(specifier).replace(/^node:/, "") === "tty") {
    return __quenchTtyModule;
  }
  return __quenchOriginalRequireWithTty(specifier);
};
"#);
