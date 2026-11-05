function Link(next) { this.next = next; }
function read(link) { return link.next; }
function write(link, value) { link.next = value; }
const node = { marker: 11 };
const box = new Link(null);
let count = 0;
function check(value) {
  write(box, value);
  const held = read(box);
  if (!Object.is(held, value) || typeof held !== typeof value)
    throw new Error("slot value/type mismatch");
  if (held != null && held.marker !== value.marker)
    throw new Error("slot property mismatch");
  count++;
}
for (let i = 0; i < 3000; i++) check(i & 1 ? node : null);
for (const value of [undefined, 1, 0.5, -0, NaN, "x", true, [], function () {}, node, null, undefined])
  check(value);
for (let i = 0; i < 64; i++) check(i & 1 ? node : null);
globalThis.answer = count;
