'use strict';
const assert = require('node:assert');
const fs = require('node:fs');
const fsp = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');
const { pathToFileURL } = require('node:url');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'quench-glob-policy-'));
const normalize = entries => entries.map(entry => typeof entry === 'string' ? entry : {
  name: entry.name,
  parentPath: path.relative(root, entry.parentPath),
  file: entry.isFile(), directory: entry.isDirectory(), link: entry.isSymbolicLink(),
  instance: entry instanceof fs.Dirent,
}).sort((a,b) => JSON.stringify(a).localeCompare(JSON.stringify(b)));
(async () => {
  try {
    fs.mkdirSync(path.join(root, 'nested'));
    fs.mkdirSync(path.join(root, 'outside'));
    for (const name of ['a.txt', 'b.js', '.hidden', 'nested/c.txt', 'nested/d.js', 'outside/secret.txt']) fs.writeFileSync(path.join(root,name), name);
    fs.symlinkSync(path.join(root, 'outside'), path.join(root, 'nested', 'link'), 'dir');
    const regularFile = path.join(root, 'a.txt');
    const checkScandir = error => {
      assert.strictEqual(error.code, 'ENOTDIR');
      assert.strictEqual(error.syscall, 'scandir');
      assert.strictEqual(error.path, regularFile);
      assert.strictEqual(error.message, `ENOTDIR: not a directory, scandir '${regularFile}'`);
      assert(error.errno < 0);
    };
    assert.throws(() => fs.readdirSync(regularFile, {withFileTypes:true}), error => {checkScandir(error);return true;});
    await new Promise((resolve,reject) => fs.readdir(regularFile, {withFileTypes:true}, error => {try{checkScandir(error);resolve();}catch(e){reject(e)}}));

    for (const [pattern, extra] of [
      ['**/*.txt', {}], [['*.js','nested/*.txt'], {}],
      ['**/*', {exclude:['outside/**']}], ['**/*', {withFileTypes:true, followSymlinks:false}],
      ['**/*', {exclude: name => name.includes('outside')}],
    ]) {
      const options = {cwd: root, ...extra};
      const sync = normalize(fs.globSync(pattern, options));
      const callback = await new Promise((resolve,reject) => fs.glob(pattern, options, (error, values) => error ? reject(error) : resolve(normalize(values))));
      const promised = [];
      for await (const entry of fsp.glob(pattern, options)) promised.push(entry);
      assert.deepStrictEqual(callback, sync);
      assert.deepStrictEqual(normalize(promised), sync);
      console.log(JSON.stringify(sync));
    }
    assert.deepStrictEqual(fs.globSync('*.txt', {cwd:pathToFileURL(root + path.sep)}), ['a.txt']);
    for (const name of ['glob','globSync']) {
      const descriptor = Object.getOwnPropertyDescriptor(fs,name);
      console.log(JSON.stringify([name,fs[name].name,fs[name].length,descriptor.writable,descriptor.enumerable,descriptor.configurable]));
    }
    const dirents = fs.readdirSync(root, {withFileTypes:true});
    assert(dirents.every(entry => entry instanceof fs.Dirent && entry.parentPath === root));
    const link = fs.readdirSync(path.join(root,'nested'), {withFileTypes:true}).find(entry=>entry.name==='link');
    assert(link.isSymbolicLink()); assert(!link.isDirectory());
    Object.defineProperty(Object.prototype, 'type', {configurable:true,set(){throw new Error('private entry type leaked')}});
    try {assert(fs.readdirSync(root, {withFileTypes:true}).every(entry=>entry instanceof fs.Dirent));}
    finally {delete Object.prototype.type;}
    const dir = fs.opendirSync(root);
    try {let entry; while((entry=dir.readSync())!==null) assert(entry instanceof fs.Dirent && entry.parentPath === root);} finally {dir.closeSync();}
    for(const args of [[null],[42],[{}],[[1]],['*',{exclude:42}],['*',{followSymlinks:1}]]) {
      try {fs.globSync(...args);throw new Error('expected rejection');}
      catch(error) {assert.strictEqual(error.code,'ERR_INVALID_ARG_TYPE'); console.log(JSON.stringify([error.name,error.code,error.message]));}
    }
    let synchronous = true;
    const completion = new Promise((resolve,reject)=>fs.glob('*.txt',{cwd:root},(error,entries)=>{try{assert(!synchronous);if(error)throw error;assert.deepStrictEqual(entries,['a.txt']);resolve();}catch(e){reject(e)}}));
    synchronous=false; await completion;
    console.log('glob-policy-ok');
  } finally {fs.rmSync(root,{recursive:true,force:true});}
})().catch(error=>{console.error(error);process.exitCode=1;});
