import {execaNode} from '/workspace/quench/tests/npm-top300/node_modules/execa/index.js';
try{console.log((await execaNode('/workspace/scratch/execa-node-child.cjs',['node-child'])).stdout)}
catch(error){console.log(JSON.stringify({complete:true,message:error.message,cause:error.cause?.stack,stack:error.stack}));}
