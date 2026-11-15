// Differential scenarios for proved field regions (`vm::dispatch_fast::field_region`): the specialized lane and the general path print the same lines.
const out = [];
const log = (name, v) => out.push(name + " " + (typeof v === "string" ? v : Object.is(v, -0) ? "-0" : String(v)));

function Pt(x, y) { this.x = x; this.y = y; }
function fields(p, n) { let s = 0; for (let i = 0; i < n; i++) { p.x = (p.x + p.y) | 0; s = (s + p.x) | 0; } return s + "," + p.x + "," + p.y; }
function readOnly(p, n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + p.x + p.y) | 0; } return s; }
function scaled(p, n) { let s = 0; for (let i = 0; i < n; i++) { p.y = (p.y + i * 3) | 0; s = (s + p.y) | 0; } return s + "," + p.y; }

// warm on one shape, then vary what the entry proves
for (let k = 0; k < 40; k++) fields(new Pt(1, 2), 30);
log("plain", fields(new Pt(1, 2), 50));
log("plain 31-bit edge", fields(new Pt(1 << 29, 1 << 29), 5));
log("plain 31-bit overflow", fields(new Pt(1 << 30, 1 << 30), 5));
log("plain negative", fields(new Pt(-5, -7), 20));
log("int32 max", fields(new Pt(2147483647, 1), 5));
log("double field", fields(new Pt(1.5, 2), 5));
log("double y", fields(new Pt(1, 0.25), 5));
log("string field", fields(new Pt("a", 2), 3));
log("undefined field", fields(new Pt(undefined, 2), 3));
log("object valueOf", fields(new Pt({ valueOf() { return 4; } }, 2), 3));
log("zero iterations", fields(new Pt(3, 4), 0));
// the same shape, many entries: proof reused and re-proved
let p = new Pt(1, 1);
for (let k = 0; k < 100; k++) fields(p, 7);
log("reused", fields(p, 7));
// a receiver of another shape
function Other(y, x) { this.y = y; this.x = x; }
log("other shape", fields(new Other(2, 1), 20));
log("other shape again", fields(new Pt(1, 2), 20));
function Triple(x, y, z) { this.x = x; this.y = y; this.z = z; }
log("three fields", fields(new Triple(1, 2, 3), 20));
const lit = { x: 1, y: 2 };
log("literal", fields(lit, 20));
const extra = new Pt(1, 2); extra.z = 9;
log("extra field", fields(extra, 20));
// deleting a field before and between entries
const gone = new Pt(1, 2); delete gone.y;
log("deleted y", fields(gone, 3));
const deleted = new Pt(1, 2); delete deleted.x; deleted.x = 5;
log("deleted x re-added", fields(deleted, 3));
// accessors installed later
const accessor = new Pt(1, 2);
for (let k = 0; k < 40; k++) fields(new Pt(1, 2), 5);
Object.defineProperty(accessor, "x", { get() { return 4; }, set(v) { log("setter", v); }, configurable: true });
log("accessor", fields(accessor, 3));
let seen = 0;
Object.defineProperty(Pt.prototype, "w", { get() { seen++; return 1; }, configurable: true });
log("prototype accessor", fields(new Pt(1, 2), 4) + " " + seen);
// frozen and non-writable
const frozen = Object.freeze(new Pt(1, 2));
log("frozen", fields(frozen, 3));
const readonly = new Pt(1, 2); Object.defineProperty(readonly, "x", { writable: false });
log("readonly", fields(readonly, 3));
// a setter on the prototype chain cannot see an own data property
Object.defineProperty(Pt.prototype, "q", { set(v) { log("proto setter", v); }, configurable: true });
log("proto setter present", fields(new Pt(2, 3), 3));
// exceptions thrown inside the loop
function thrower(p, n, at) { let s = 0; try { for (let i = 0; i < n; i++) { p.x = (p.x + p.y) | 0; if (i === at) throw new Error("e" + p.x); s = (s + p.x) | 0; } } catch (e) { return e.message + " " + s + " " + p.x; } return s; }
log("thrower", thrower(new Pt(1, 2), 30, 10));
log("thrower none", thrower(new Pt(1, 2), 30, 99));
// getters that throw on a non-int receiver mid-way (receiver swapped between entries)
const objects = [new Pt(1, 2), new Pt(3, 4), new Pt(5, 6)];
let total = 0;
for (let k = 0; k < 60; k++) total = (total + fields(objects[k % 3], 3).length) | 0;
log("rotating receivers", total + " " + objects.map(o => o.x + ":" + o.y).join(","));
// two receivers in one loop
function two(a, b, n) { let s = 0; for (let i = 0; i < n; i++) { a.x = (a.x + b.y) | 0; b.y = (b.y + a.x) | 0; s = (s + a.x) | 0; } return s + "," + a.x + "," + b.y; }
for (let k = 0; k < 40; k++) two(new Pt(1, 2), new Pt(3, 4), 5);
log("two receivers", two(new Pt(1, 2), new Pt(3, 4), 20));
const same = new Pt(1, 2);
log("same receiver twice", two(same, same, 20));
// reads only, with islands
for (let k = 0; k < 40; k++) readOnly(new Pt(1, 2), 30);
log("read only", readOnly(new Pt(7, 8), 100));
log("read only double", readOnly(new Pt(7, 0.5), 10));
for (let k = 0; k < 40; k++) scaled(new Pt(1, 2), 30);
log("scaled", scaled(new Pt(1, 2), 100));
log("scaled wrap", scaled(new Pt(1, 1 << 29), 100));
// the loop widens the representation mid-way: a store the narrow form cannot hold
function widen(p, n) { for (let i = 0; i < n; i++) { p.x = (p.x * 2 + 1) | 0; } return p.x; }
for (let k = 0; k < 40; k++) widen(new Pt(1, 2), 5);
log("widen", widen(new Pt(1, 2), 40));
log("widen after", widen(new Pt(1, 2), 40));
// reentrancy: a loop inside a loop over the same receiver
function nested(p, m, n) { let t = 0; for (let j = 0; j < m; j++) { for (let i = 0; i < n; i++) { p.x = (p.x + p.y) | 0; t = (t + p.x) | 0; } p.y = (p.y + 1) | 0; } return t; }
log("nested", nested(new Pt(1, 2), 20, 7));
log("nested again", nested(new Pt(1, 2), 20, 7));
// a function called through a getter-free method with this
Pt.prototype.run = function (n) { let s = 0; for (let i = 0; i < n; i++) { this.x = (this.x + this.y) | 0; s = (s + this.x) | 0; } return s; };
log("method", new Pt(1, 2).run(20));
// JSON of results
log("done", JSON.stringify(objects));
for (const line of out) print(line);
