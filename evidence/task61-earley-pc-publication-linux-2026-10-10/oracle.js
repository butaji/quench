const rows = [];
function record(name, fn) { try { rows.push([name, 'return', fn()]); } catch (e) { rows.push([name, 'throw', e.name, e.message]); } }
let getterCount = 0;
const object = { get value() { getterCount++; return 7; } };
record('getter_and_finally', () => { let state = 0; try { return object.value; } finally { state++; } });
rows.push(['getter_count', getterCount]);
const thrown = { marker: 'same-object' };
record('caught_identity', () => { try { throw thrown; } catch (e) { return e === thrown; } });
let proxyGets = 0;
const proxy = new Proxy({ value: 9 }, { get(target, key, receiver) { proxyGets++; return Reflect.get(target, key, receiver); } });
record('proxy_get', () => proxy.value);
rows.push(['proxy_get_count', proxyGets]);
record('nested_throw_catch', () => { function inner() { throw new RangeError('nested'); } try { inner(); } catch (e) { return [e.name, e.message]; } });
record('tdz_read_catch', () => { try { { let x = x; } } catch (e) { return e.name; } });
record('tdz_write_catch', () => { try { { let x = (x = 2); } } catch (e) { return e.name; } });
record('finally_throw', () => { let state = 0; try { throw new TypeError('finally'); } finally { state++; } });
record('allocation_pressure', () => { let sum = 0; for (let i = 0; i < 120000; i++) { const row = [i, { value: i }]; sum += row[1].value; } return sum; });
console.log(JSON.stringify(rows));
