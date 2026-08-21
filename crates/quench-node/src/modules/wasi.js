(function () {
  function WASI(options) {
    options = options || {};
    this.options = options;
    this.args = options.args || [];
    this.env = options.env || {};
    this.preopens = options.preopens || {};
    // Bun accepts returnOnExit but does not provide a native WASI proc_exit
    // implementation. Keep the documented default visible without pretending
    // that unsupported syscalls are available.
    this.returnOnExit = options.returnOnExit === undefined ? true : options.returnOnExit;
    this.wasiImport = {};
  }
  WASI.prototype.start = function (instance) {
    if (!instance || !instance.exports) throw new TypeError('instance must export _start');
    if (typeof instance.exports._start !== 'function') throw new TypeError('instance must export _start');
    return instance.exports._start();
  };
  WASI.prototype.initialize = function (instance) {
    if (!instance || !instance.exports) throw new TypeError('instance must export _initialize');
    if (typeof instance.exports._initialize === 'function') return instance.exports._initialize();
    return 0;
  };
  WASI.prototype.getImportObject = function () {
    return { wasi_snapshot_preview1: this.wasiImport };
  };
  module.exports = { WASI: WASI };
}());