'use strict';
const assert=require('assert'),fs=require('fs'),os=require('os'),path=require('path');
const stat=process.binding('fs').internalModuleStat;
const root=fs.mkdtempSync(path.join(os.tmpdir(),'quench-module-stat-'));
const cwd=process.cwd();
try {
 fs.mkdirSync(path.join(root,'directory'));
 fs.writeFileSync(path.join(root,'file'),'data');
 fs.symlinkSync('directory',path.join(root,'dir-link'));
 fs.symlinkSync('file',path.join(root,'file-link'));
 fs.symlinkSync('absent',path.join(root,'dangling'));
 const results=['directory','file','dir-link','file-link','dangling','missing','file/child'].map(name=>stat(path.join(root,name)));
 assert.deepStrictEqual(results,[1,0,1,0,-2,-2,-20]);
 assert.strictEqual(stat(root+'\0ignored'),1);
 process.chdir(root);assert.strictEqual(stat('directory'),1);assert.strictEqual(stat('file'),0);
 console.log(JSON.stringify([stat.name,stat.length,results]));
} finally {process.chdir(cwd);fs.rmSync(root,{recursive:true,force:true});}
