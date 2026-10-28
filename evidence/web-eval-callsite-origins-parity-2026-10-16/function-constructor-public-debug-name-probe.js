const u=require('util');const A=Function('this.x=1');Error.prepareStackTrace=(_,f)=>f;console.log(JSON.stringify([A.name,u.inspect(A),u.inspect(new A())]));
