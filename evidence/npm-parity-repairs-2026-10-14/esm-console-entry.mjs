
import util from 'node:util';
console.log(null,undefined,42);
console.log('%s:%d:%j','answer',42,{value:null});
util.format=()=>{throw Error('public formatter replacement was used')};
util.formatWithOptions=util.format;
console.log(null,undefined);
