const event = new Event('tick', { cancelable: true });
if (event.type !== 'tick' || event.cancelable !== true || event.defaultPrevented !== false) process.exit(1);
event.preventDefault();
if (event.defaultPrevented !== true) process.exit(1);
const target = new EventTarget();
let called = false;
const dispatched = new Event('tick');
target.addEventListener('tick', (value) => { called = value === dispatched; });
if (!target.dispatchEvent(dispatched) || !called) process.exit(1);
