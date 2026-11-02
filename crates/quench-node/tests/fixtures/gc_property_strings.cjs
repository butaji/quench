'use strict';
const symbol = Symbol('property');
function text(index) {
  return 'property-' + index + '-😀-' + String.fromCharCode(0xd800 + index % 16);
}
const roots = [];
for (let i = 0; i < 256; i++) {
  const inline = { first: text(i), second: text(i + 256) };
  const wide = {};
  for (let key = 0; key < 20; key++) wide['key' + key] = text(i * 20 + key);
  wide[symbol] = text(i + 1024);
  const prototype = { inherited: text(i + 2048) };
  const inherited = Object.create(prototype);
  inherited.own = text(i + 3072);
  const captured = text(i + 4096);
  const accessor = { get value() { return captured; } };
  roots.push({ inline, wide, inherited, accessor });
}
function digest() {
  let hash = 0;
  function add(value) {
    for (let i = 0; i < value.length; i++) hash = (hash * 31 + value.charCodeAt(i)) | 0;
  }
  for (const root of roots) {
    add(root.inline.first); add(root.inline.second);
    for (let key = 0; key < 20; key++) add(root.wide['key' + key]);
    add(root.wide[symbol]); add(root.inherited.inherited); add(root.inherited.own);
    add(root.accessor.value);
  }
  return hash;
}
console.log(digest());
for (let cycle = 0; cycle < 4; cycle++) {
  const direct = text(60000 + cycle);
  gc();
  console.log(direct.length, direct.charCodeAt(direct.length - 1), digest());
  for (let i = 0; i < roots.length; i++) {
    roots[i].inline.first = text(cycle * 10000 + i);
    roots[i].wide.key19 = text(cycle * 20000 + i);
  }
  let garbage = [];
  for (let i = 0; i < 4096; i++) garbage.push({ text: text(cycle * 30000 + i) });
  garbage = null;
  gc();
  console.log(digest());
}
