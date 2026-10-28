const c = process.config, v = c.variables;
const show = o => JSON.stringify(Object.entries(Object.getOwnPropertyDescriptors(o)).slice(0, 3).map(([k, d]) => [k, d.writable, d.enumerable, d.configurable]));
console.log(Object.isFrozen(c), Object.isFrozen(v), Object.isExtensible(c), show(c), show(v), Object.keys(v).length, Object.getPrototypeOf(v) === Object.prototype);
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(process, 'config')));
