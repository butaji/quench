'use strict';
const assert = require('assert');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { pathToFileURL } = require('url');
const { internalBinding } = require('internal/test/binding');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'quench-readdir-'));
const inspect = value => Buffer.isBuffer(value) ? ['buffer', value.toString('hex')] : value;
try {
  fs.mkdirSync(path.join(root, 'b'));
  fs.mkdirSync(path.join(root, 'a'));
  fs.mkdirSync(path.join(root, 'a', 'deep'));
  fs.writeFileSync(path.join(root, 'z.txt'), '');
  fs.writeFileSync(path.join(root, 'a', 'one'), '');
  fs.writeFileSync(path.join(root, 'b', 'two'), '');
  fs.writeFileSync(path.join(root, 'a', 'deep', 'three'), '');
  fs.writeFileSync(path.join(root, 'é😀'), '');
  if (process.platform !== 'win32') {
    fs.symlinkSync('a', path.join(root, 'link'));
    fs.symlinkSync('missing', path.join(root, 'broken'));
    fs.closeSync(fs.openSync(Buffer.concat([Buffer.from(root + '/'), Buffer.from([0xff, 0xfe])]), 'w'));
  }
  const rows = [];
  for (const encoding of ['utf8', 'buffer', 'hex', 'latin1', 'ascii', 'utf16le', 'base64']) {
    for (const recursive of [false, true]) {
      rows.push([encoding, recursive, fs.readdirSync(root, { encoding, recursive }).map(inspect)]);
    }
  }
  for (const input of [root, Buffer.from(root), pathToFileURL(root)]) {
    const entries = fs.readdirSync(input, { withFileTypes: true, recursive: true });
    rows.push(entries.map(entry => [inspect(entry.name), path.relative(root, Buffer.isBuffer(entry.parentPath) ? entry.parentPath.toString() : entry.parentPath),
      entry instanceof fs.Dirent, entry.isFile(), entry.isDirectory(), entry.isSymbolicLink(),
      entry.isFIFO(), entry.isSocket(), entry.isCharacterDevice(), entry.isBlockDevice()]));
  }
  for (const recursive of [false, true]) {
    rows.push(fs.readdirSync(root, { withFileTypes: true, recursive, encoding: 'buffer' }).map(entry =>
      [inspect(entry.name), inspect(Buffer.isBuffer(entry.parentPath) ? Buffer.from(path.relative(root, entry.parentPath.toString())) : path.relative(root, Buffer.isBuffer(entry.parentPath) ? entry.parentPath.toString() : entry.parentPath)), entry.isDirectory()]));
  }
  for (const options of [{recursive:1}, {encoding:'wat'}, 3, {recursive:null}]) {
    try { fs.readdirSync(root, options); rows.push('ok'); }
    catch (e) { rows.push([e.name, e.code, e.message]); }
  }
  for (const input of ['', Buffer.alloc(0), path.join(root,'absent'), path.join(root,'z.txt')]) {
    try { fs.readdirSync(input); assert.fail('must fail'); }
    catch (e) { rows.push([e.name,e.code,e.errno,e.syscall,e.path === '' ? '' : path.relative(root,e.path)]); }
  }
  const access = [];
  const options = {
    get encoding() { access.push('encoding'); return 'utf8'; },
    get recursive() { access.push('recursive'); return false; },
    get withFileTypes() { access.push('withFileTypes'); return true; },
  };
  rows.push(fs.readdirSync(root, options).map(entry => entry.name));
  rows.push(access);
  const mutablePath = Buffer.from(path.join(root, 'a'));
  rows.push(fs.readdirSync(mutablePath, { get encoding() {
    mutablePath[mutablePath.length - 1] = 'b'.charCodeAt(0); return 'utf8';
  }}));
  const sentinel = new Error('option getter');
  try { fs.readdirSync(1, {get encoding() {throw sentinel;}}); assert.fail('getter must throw'); }
  catch (e) { assert.strictEqual(e, sentinel); }
  const binding = internalBinding('fs');
  rows.push([binding.readdir.name, binding.readdir.length, binding.readdirRecursive.name,
    binding.readdirRecursive.length, binding.readdir(root, 'utf8', 1).every(name => typeof name === 'string')]);
  const packed = binding.readdirRecursive(root, 'utf8', true);
  assert(packed[1] instanceof Uint8Array);
  assert(packed[2] instanceof Uint32Array);
  assert.strictEqual(packed[1].buffer, packed[2].buffer);
  rows.push([Array.from(packed[1]), Array.from(packed[2]), packed[3]]);
  const descriptor = Object.getOwnPropertyDescriptor(fs, 'readdirSync');
  rows.push([fs.readdirSync.name, fs.readdirSync.length, descriptor.writable,
    descriptor.enumerable, descriptor.configurable, Object.hasOwn(fs.readdirSync, 'prototype')]);
  const originalU8 = globalThis.Uint8Array;
  const originalU32 = globalThis.Uint32Array;
  try {
    globalThis.Uint8Array = globalThis.Uint32Array = function() { throw new Error('public constructor invoked'); };
    rows.push(fs.readdirSync(root, {recursive:true, withFileTypes:true}).map(entry => entry.name));
  } finally { globalThis.Uint8Array = originalU8; globalThis.Uint32Array = originalU32; }
  console.log(JSON.stringify(rows));
} finally { fs.rmSync(root, { recursive: true, force: true }); }
