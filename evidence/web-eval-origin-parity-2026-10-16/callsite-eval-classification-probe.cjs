Error.prepareStackTrace=(_,frames)=>frames;
function capture(){return new Error().stack.slice(0,2).map(f=>f.isEval())}
function direct(){return eval('capture()')}
console.log(JSON.stringify([direct(),(0,eval)('capture()'),Function('return capture()')()]));
