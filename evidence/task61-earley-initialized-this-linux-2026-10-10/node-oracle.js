const seen = [];
class Base {
  constructor(v) { this.base = v; }
}
class Derived extends Base {
  constructor(v) {
    const local = v;
    try { this.before = local; }
    catch (error) { seen.push(['pre-super', error.name]); }
    super(local);
    this.after = local;
    const read = () => eval('this');
    seen.push(['eval-after-super', eval('this') === this]);
    seen.push(['arrow-after-super', read() === this]);
    seen.push(['base-value', this.base === local, this.after === local]);
  }
}
for (const value of [undefined, null, 0, 'v', { marker: 41 }]) new Derived(value);
function strictWrite(value) {
  'use strict';
  this.field = value;
}
const receiver = {};
strictWrite.call(receiver, 17);
seen.push(['strict-receiver', receiver.field === 17]);
try { strictWrite.call(undefined, 19); }
catch (error) { seen.push(['undefined-receiver', error.name]); }
class Throwing { set field(value) { throw new Error(`setter:${value}`); } }
function strictSetter(value) {
  'use strict';
  const local = value;
  this.field = local;
}
try { strictSetter.call(new Throwing(), 'x'); }
catch (error) { seen.push(['setter-throw', error.message]); }
console.log(JSON.stringify(seen));
