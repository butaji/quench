function Base(value) {
  this.base = value;
}

function Derived(value) {
  this.own = value + 1;
}
Derived.prototype = new Base(-1);

let last;
for (let i = 0; i < 100000; i++) last = new Derived(i);
if (last.base !== -1 || last.own !== 100000) {
  throw new Error("derived constructor transition lost a field");
}
