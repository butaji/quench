const fs=require('node:fs'),os=require('node:os'),path=require('node:path'),out=[];
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-write-policy-')),file=path.join(dir,'file');
function error(e){return ['error',e.name,e.code??null,e.syscall??null,e.message,Reflect.ownKeys(e).filter(k=>k!=='stack').map(String),Reflect.ownKeys(Object.getPrototypeOf(e)).map(String)]}
try{
 for(const kind of ['Uint8Array','Uint16Array','Int32Array','DataView']){
  fs.writeFileSync(file,'--------');const fd=fs.openSync(file,'r+'),backing=new ArrayBuffer(12),view=kind==='DataView'?new DataView(backing,4,8):new globalThis[kind](backing,4,8/globalThis[kind].BYTES_PER_ELEMENT);new Uint8Array(backing).set([65,66,67,68,69,70,71,72],4);
  for(const name of ['buffer','byteOffset','subarray'])Object.defineProperty(view,name,{get(){throw new Error('public '+name)}});
  try{out.push([kind,fs.writeSync(fd,view,2,4,1),fs.readFileSync(file,'utf8')])}catch(e){out.push([kind,error(e)])}finally{fs.closeSync(fd)}
 }
 for(const position of [undefined,null,-1,-2,0,1,1.5,NaN,Infinity,Number.MAX_SAFE_INTEGER+1,'2',2n,{}]){
  fs.writeFileSync(file,'abcdef');const fd=fs.openSync(file,'r+');try{fs.readSync(fd,Buffer.alloc(3),0,3,null);out.push(['position',typeof position,(()=>{try{return fs.writeSync(fd,'X',position,'utf8')}catch(e){return error(e)}})(),fs.readFileSync(file,'utf8'),fs.readFileSync(fd,'utf8')])}finally{fs.closeSync(fd)}
 }
 for(const data of [{},null,1,new Uint8Array(0),'x'])for(const fd of [-1,{},999999])try{out.push(['fd',fs.writeSync(fd,data,0,0,0)])}catch(e){out.push(error(e))}
 for(const encoding of [undefined,null,'','bad','buffer','hex','HEX',1,{},Symbol('encoding')]){
  fs.writeFileSync(file,'');const fd=fs.openSync(file,'r+');try{out.push(['encoding',(()=>{try{return fs.writeSync(fd,'éA',null,encoding)}catch(e){return error(e)}})(),Array.from(fs.readFileSync(file))])}finally{fs.closeSync(fd)}
 }
}finally{fs.rmSync(dir,{recursive:true,force:true})}console.log(JSON.stringify(out));
