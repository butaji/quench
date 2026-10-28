const fs=require('node:fs');
for(const bigint of [false,true]){const st=fs.statfsSync('/',{bigint}),p=Object.getPrototypeOf(st),d=Object.getOwnPropertyDescriptor(p,'constructor');console.log(JSON.stringify([bigint,st.constructor.name,st.constructor.length,Reflect.ownKeys(p).map(String),d.writable,d.enumerable,d.configurable,Object.keys(st),typeof st.blocks]));}
