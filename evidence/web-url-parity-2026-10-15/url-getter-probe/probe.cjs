const {fileURLToPath}=require('node:url');
for(const make of [()=>{const u=new URL('file:///');Object.defineProperty(u,'protocol',{get(){throw 'protocol-marker'}});return u},()=>({href:'file:///',protocol:'file:',hostname:'',pathname:'/tmp'}),()=>new URL('file:///%ZZ')]){try{console.log('value',fileURLToPath(make()))}catch(e){console.log('error',typeof e,e?.name,e?.code,e?.message??e)}}
