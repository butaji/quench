//! Polyfill: `directory`

pub const JS: &str = r#"globalThis.__nodeFs.Dirent = class Dirent {
  constructor(name, type = 1) {
    this.name = name;
    this._type = type === true ? 2 : type === false ? 1 : type;
  }
  isFile() {
    return this._type === 0 || this._type === 1;
  }
  isDirectory() {
    return this._type === 2;
  }
  isSymbolicLink() {
    return this._type === 3;
  }
  isFIFO() {
    return this._type === 4;
  }
  isSocket() {
    return this._type === 5;
  }
  isCharacterDevice() {
    return this._type === 6;
  }
  isBlockDevice() {
    return this._type === 7;
  }
};
Object.defineProperty(globalThis.__nodeFs.Dir.prototype, "path", {
  get() {
    if (
      this === globalThis.__nodeFs.Dir.prototype ||
      !(this instanceof globalThis.__nodeFs.Dir)
    ) {
      throw Object.assign(new TypeError("Method get path called on incompatible receiver"), { code: "ERR_INVALID_THIS" });
    }
    return this._path;
  },
});
"#;
