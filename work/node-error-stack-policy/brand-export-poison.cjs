const errors=require('internal/errors'),original=errors.kIsNodeError;
errors.kIsNodeError=Symbol('replacement');
try{require('fs').readFileSync(null)}catch(e){console.log(e[original],e[errors.kIsNodeError],e.stack.split('\n')[0])}
