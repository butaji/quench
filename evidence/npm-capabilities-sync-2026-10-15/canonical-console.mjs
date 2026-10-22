
import util from 'node:util';
util.format=()=>{throw Error('public formatter replacement was used')};
util.formatWithOptions=util.format;
console.log(null,undefined,42);
console.log('%s:%d:%j','answer',42,{value:null});
console.log(null,undefined);
