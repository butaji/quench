const seen = [];
class Base {
  set value(v) { seen.push(['setter', v]); }
  get value() { return seen.at(-1)?.[1]; }
}
class Child extends Base {
  constructor(v) {
    const local = v;
    super();
    this.value = local;
    this.after = this.value;
  }
}
for (const value of [undefined, null, false, 0, 17, 'q', { marker: 29 }]) {
  const x = new Child(value);
  seen.push(['result', x.after === value, x.value === value]);
}
class BadBase { set value(v) { throw new Error(`blocked:${v}`); } }
class BadChild extends BadBase {
  constructor(v) {
    const local = v;
    super();
    try { this.value = local; } catch (error) { seen.push(['throw', error.message]); }
  }
}
new BadChild('strict');
class Derived extends Base {
  constructor(v) {
    const local = v;
    try { this.value = local; } catch (error) { seen.push(['pre-super', error.name]); }
    super();
    this.value = local;
  }
}
new Derived('after-super');
console.log(JSON.stringify(seen));
