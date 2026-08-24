const event = new Event('tick', { cancelable: true });
if (event.type !== 'tick' || event.cancelable !== true || event.defaultPrevented !== false) process.exit(1);
event.preventDefault();
if (event.defaultPrevented !== true) process.exit(1);
const target = new EventTarget();
let called = false;
target.addEventListener('tick', (value) => { called = value === event; });
if (!target.dispatchEvent(event) || !called) process.exit(1);
