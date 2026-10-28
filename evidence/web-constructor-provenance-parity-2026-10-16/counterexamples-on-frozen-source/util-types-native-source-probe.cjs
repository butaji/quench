console.log(JSON.stringify(Object.entries(require('util').types).filter(([_,v])=>typeof v==='function').map(([k,v])=>[k,v.name,v.length,Function.prototype.toString.call(v)])));
