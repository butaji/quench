const view=new DataView(new ArrayBuffer(8));
const fromBits=bits=>{view.setBigUint64(0,bits);return view.getFloat64(0);};
const toBits=number=>{view.setFloat64(0,number);return view.getBigUint64(0).toString(16).padStart(16,'0');};
let state=0x71317a951cfb028dn;
const next=()=>{state=BigInt.asUintN(64,state*6364136223846793005n+1442695040888963407n);return fromBits((state&0x7ff0000000000000n)===0x7ff0000000000000n?state^0x0010000000000000n:state);};
const rows=[];
const add=(name,args)=>rows.push({name,args:args.map(toBits),result:toBits(Math[name](...args))});
for(const x of [0,-0,Infinity,-Infinity,NaN,Number.MIN_VALUE,-Number.MIN_VALUE,Number.MAX_VALUE,-Number.MAX_VALUE,1,-1,8,-8])add('cbrt',[x]);
for(const x of [0,-0,Infinity,-Infinity,NaN,1,-1])for(const y of [0,-0,Infinity,-Infinity,NaN,1,-1])add('atan2',[x,y]);
for(let i=0;i<10000;i++){add('cbrt',[next()]);add('atan',[next()]);add('atan2',[next(),next()]);}
for(let i=-1000;i<=1000;i++){const x=(i+0.5)/100;add('atan',[x]);add('atan2',[x,-1]);add('cbrt',[x]);}
console.log(JSON.stringify({complete:true,cases:rows.length,rows}));
