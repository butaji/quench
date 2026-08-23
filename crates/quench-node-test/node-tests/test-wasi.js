// Node compat: WASI options and lifecycle surface.
const { WASI } = require('node:wasi');
const wasi = new WASI({ args: ['app'], env: { MODE: 'test' }, preopens: {}, returnOnExit: false });
if (!Array.isArray(wasi.args) || wasi.args[0] !== 'app') throw new Error('args');
if (wasi.env.MODE !== 'test') throw new Error('env');
if (wasi.returnOnExit !== false) throw new Error('returnOnExit');
const imports = wasi.getImportObject();
if (imports.wasi_snapshot_preview1 !== wasi.wasiImport) throw new Error('import object');
const result = wasi.start({ exports: { _start() { return 7; } } });
if (result !== 7) throw new Error('start result: ' + result);
console.log('wasi: options + start ok');
