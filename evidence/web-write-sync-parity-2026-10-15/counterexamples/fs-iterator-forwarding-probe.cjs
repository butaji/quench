const fs=require('node:fs'),os=require('node:os'),path=require('node:path');const directory=fs.mkdtempSync(path.join(os.tmpdir(),'quench-fs-iterator-')),file=path.join(directory,'file');fs.writeFileSync(file,'---');
const fd=fs.openSync(file,'r+'),source=Buffer.from('Q'),target=Buffer.alloc(1),prototype=Object.getPrototypeOf([][Symbol.iterator]()),descriptor=Object.getOwnPropertyDescriptor(prototype,'next'),marker={},out=[];
try {
 Object.defineProperty(prototype,'next',{...descriptor,value(){throw marker}});
 try{out[0]=fs.writeSync(fd,source,0,1,0)}catch(error){out[0]=error===marker?'iterator-marker':error.message}
 try{out[1]=fs.readSync(fd,target,0,1,0)}catch(error){out[1]=error===marker?'iterator-marker':error.message}
}finally{Object.defineProperty(prototype,'next',descriptor);fs.closeSync(fd);fs.rmSync(directory,{recursive:true,force:true})}
out[2]=target[0];console.log(JSON.stringify(out));
