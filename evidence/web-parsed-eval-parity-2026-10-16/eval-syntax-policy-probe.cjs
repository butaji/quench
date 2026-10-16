const inputs=['"for(;false;)"','"import x"','"return value"','#!/usr/bin/env node\nMath.max(2,3)','1;\n\nnew Error().stack'];
console.log(JSON.stringify(inputs.map(source=>{try{return {value:eval(source)}}catch(error){return {error:error.name}}})));
