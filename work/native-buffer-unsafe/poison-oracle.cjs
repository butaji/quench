const v=require('internal/validators'),codes=require('internal/errors').codes;
for(const key of ['validateNumber','validateInteger','validateArray','validateString'])v[key]=()=>{throw new Error('poisoned '+key)};
for(const key of ['ERR_INVALID_ARG_VALUE','ERR_OUT_OF_RANGE','ERR_INVALID_ARG_TYPE'])codes[key]=function(){throw new Error('poisoned '+key)};
console.log('valid',Buffer.allocUnsafe(3).length,Buffer.allocUnsafeSlow(3,64).length,Buffer.concat([new Uint8Array([1])]).toString('hex'),require('buffer').isLatin1('abc'));
for(const call of [()=>Buffer.allocUnsafe(-1),()=>Buffer.allocUnsafe(3,3),()=>Buffer.concat([1]),()=>require('buffer').isLatin1(1)]){try{call()}catch(e){console.log('error',e.name,e.code,e.message)}}
