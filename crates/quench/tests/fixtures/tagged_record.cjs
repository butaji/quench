function make(expected) {
  'use strict';
  return function update(task) {
    var node = task.node;
    if ('object' === typeof node && null !== node)
      switch (node.kind) {
        case expected:
          task.stack = { parent: task.stack, type: node.type };
      }
  };
}
const tag = Symbol('tag');
const update = make(tag);
const task = { node: { kind: tag, type: 'initial' }, stack: null };
for (let cycle = 0; cycle < 3; cycle++) {
  for (let i = 0; i < 100; i++) {
    task.node.type = cycle + ':' + i;
    const previous = task.stack;
    if (update(task) !== undefined || task.stack.parent !== previous)
      throw Error('update completion or parent identity');
  }
  gc();
}
let count = 0;
for (let item = task.stack; item !== null; item = item.parent) count++;
console.log(JSON.stringify([count, task.stack.type, Object.keys(task.stack)]));
for (const node of [null, undefined, false, 0, 'text', () => {}, {}, { kind: Symbol('tag') }]) {
  const previous = task.stack;
  task.node = node;
  console.log(JSON.stringify([update(task), task.stack === previous]));
}
for (const expected of [NaN, 0, 'same', 12n, Symbol.for('shared'), {}]) {
  const run = make(expected);
  const subject = { node: { kind: expected, type: 'payload' }, stack: null };
  for (let i = 0; i < 80; i++) run(subject);
  console.log(JSON.stringify([String(expected), subject.stack === null,
    subject.stack && subject.stack.type]));
}
const events = [];
const node = { kind: tag, type: 'from-getter' };
const withGetter = { get node() { events.push('node'); return node; }, stack: null };
update(withGetter);
const withNodeGetters = {
  node: { get kind() { events.push('kind'); return tag; }, get type() { events.push('type'); return 'accessor'; } },
  get stack() { events.push('read-stack'); return null; },
  set stack(value) { events.push('write-stack:' + value.type); }
};
update(withNodeGetters);
const proxy = new Proxy({ node, stack: null }, {
  get(target, key, receiver) { events.push('get:' + String(key)); return Reflect.get(target, key, receiver); },
  set(target, key, value, receiver) { events.push('set:' + String(key)); return Reflect.set(target, key, value, receiver); }
});
update(proxy);
for (let i = 0; i < 80; i++) {
  update(withGetter);
  update(withNodeGetters);
  update(proxy);
}
console.log(JSON.stringify(events));
task.node = node;
Object.defineProperty(task, 'stack', { writable: false });
try { update(task); } catch (error) { console.log(error.name); }
const fault = {};
const throwing = { get node() { throw fault; } };
let caught = 0;
for (let i = 0; i < 80; i++) {
  try { update(throwing); } catch (error) {
    if (error !== fault) throw Error('throw identity');
    caught++;
  }
}
console.log(caught);
const before = { node: null, stack: null };
function early(task) {
  var node = task.node;
  if ('object' === typeof node && null !== node)
    switch (node.kind) { case later: task.stack = { parent: task.stack, type: node.type }; }
}
console.log(early(before) === undefined);
before.node = { kind: tag, type: 'after-init' };
try { early(before); } catch (error) { console.log(error.name); }
let later = tag;
for (let i = 0; i < 80; i++) early(before);
gc();
early(before);
console.log(before.stack.type);

function makeSloppy(expected) {
  return function update(task) {
    var node = task.node;
    if ('object' === typeof node && null !== node)
      switch (node.kind) {
        case expected: task.stack = { parent: task.stack, type: node.type };
      }
  };
}
const sloppy = makeSloppy(tag);
const subject = { node: { kind: tag, type: 'sloppy' }, stack: null };
for (let i = 0; i < 80; i++) sloppy(subject);
const saved = subject.stack;
Object.defineProperty(subject, 'stack', { writable: false });
console.log(JSON.stringify([sloppy(subject), subject.stack === saved]));
