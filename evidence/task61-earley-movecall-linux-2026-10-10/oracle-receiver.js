const receiver = { value: 10 };
function incrementThis(value) { this.value += value; return this.value; }
function callWithReceiver(fn, target, value) { return fn.call(target, value); }
console.log(callWithReceiver(incrementThis, receiver, 3), receiver.value);
