const fs=require('node:fs'),os=require('node:os'),path=require('node:path');
const dir=fs.mkdtempSync(path.join(os.tmpdir(),'quench-fs-library-collision-'));
try{fs.mkdirSync(path.join(dir,'internal/fs'),{recursive:true});const file=path.join(dir,'internal/fs/utils.js');fs.writeFileSync(file,'module.exports={applicationValue:17};');const m=require(file);console.log(JSON.stringify([m.applicationValue,typeof fs.statSync(dir,{bigint:true}).size,fs.statfsSync('/').constructor.name]));}finally{fs.rmSync(dir,{recursive:true,force:true});}
