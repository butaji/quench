const out=[];
for(const [name,prefix]of [['NUL',String.fromCharCode(0)],['backslash-zero',String.fromCharCode(92,48)]]){
 const key=prefix+'quench:user-property',object={[key]:1};
 out.push([name,key.charCodeAt(0),Reflect.ownKeys(object),Object.keys(object),Object.getOwnPropertyNames(object),JSON.stringify(object)]);
}
console.log(JSON.stringify(out));
