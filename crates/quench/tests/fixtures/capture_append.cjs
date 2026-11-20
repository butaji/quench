"use strict";
const out = [];
function make(initial) {
  let text = initial;
  return {
    push: function(chunk) { null !== chunk && (text += chunk); return !0; },
    read() { return text; },
    set(value) { text = value; }
  };
}
for (let repeat = 0; repeat < 3; repeat++) {
  const sink = make("");
  for (let index = 0; index < 80; index++) sink.push("a😀");
  out.push(sink.read().length, sink.read().slice(-3), sink.push(null));
  gc();
  sink.push("!");
  out.push(sink.read().length);
}
const a = make("");
for (const value of ["x", 12, null, undefined, true, 3n]) out.push(a.push(value));
out.push(a.read());
const hints = [];
const b = make("L");
b.push({ [Symbol.toPrimitive](hint) { hints.push(hint); b.push("inner"); return "R"; } });
out.push(b.read());
b.set({ [Symbol.toPrimitive](hint) { hints.push(hint); return "Q"; } });
b.push("!");
out.push(b.read(), hints.join(","));
try { b.push(Symbol("bad")); } catch (error) { out.push(error.name, b.read()); }
const marker = {};
try { b.push({ [Symbol.toPrimitive]() { throw marker; } }); }
catch (error) { out.push(error === marker, b.read()); }
function constant() {
  const text = "C";
  return function(chunk) { null !== chunk && (text += chunk); return !0; };
}
const immutable = constant();
out.push(immutable(null));
try { immutable("x"); } catch (error) { out.push(error.name); }
function tdz() {
  const push = function(chunk) { null !== chunk && (text += chunk); return !0; };
  out.push(push(null));
  try { push("x"); } catch (error) { out.push(error.name); }
  let text = "T";
  out.push(push("x"), text);
  return push;
}
const late = tdz();
out.push(late("!"));
const numeric = make(7);
numeric.push(3);
out.push(numeric.read());
numeric.push("z");
out.push(numeric.read());
const before = make("old"), after = make("new");
const object = { push: before.push };
object.push("1"); object.push = after.push; object.push("2");
out.push(before.read(), after.read());
console.log(JSON.stringify(out));

(function () {

          function make() {
            let text = "";
            return {
              push: function(chunk, index, array) { null !== chunk && (text += chunk); return !0; },
              read() { return text; }
            };
          }
          const sink = make();
          const out = [sink.push(null), ["A", "B"].every(sink.push), sink.read()];
          const constructed = new sink.push("C");
          out.push(typeof constructed, constructed instanceof sink.push, sink.read());
          sink.push.call(null, "D");
          Reflect.apply(sink.push, undefined, ["E"]);
          out.push(sink.read());
          console.log(JSON.stringify(out));

})();

(function () {
  let text = '', effects = [];
  function guarded(chunk) {
    try { null !== chunk && (text += chunk); return true; }
    finally { effects.push('finally'); }
  }
  function caught(chunk) {
    try { null !== chunk && (text += chunk); return true; }
    catch (error) { effects.push(error.name); return false; }
  }
  console.log(JSON.stringify([guarded('A'), guarded(null), caught(Symbol('bad')), text, effects]));
})();
