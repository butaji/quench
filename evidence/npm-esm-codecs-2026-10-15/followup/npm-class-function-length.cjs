const rows=[];
function record(name,value){rows.push([name,value.name,Object.getOwnPropertyDescriptor(value,'length')]);}
for(const length of [0,1,2,3]){
 const parent = [class {},class {constructor(a){}},class {constructor(a,b){}},class {constructor(a,b,c){}}][length];
 const derived=class extends parent {};
 const explicit=class extends parent {constructor(a,b=1,...rest){super(a,b,...rest);}};
 record('parent'+length,parent);record('derived'+length,derived);record('explicit'+length,explicit);
 const result=new derived(1,2,3); rows.push(['prototype'+length,Object.getPrototypeOf(result)===derived.prototype]);
}
record('baseDefault',class {constructor(a=1){}});
record('baseRest',class {constructor(...rest){}});
record('derivedRest',class extends Function {constructor(...rest){super(...rest);}});
record('defaultDerived',class extends null {});
console.log(JSON.stringify({complete:true,rows}));
