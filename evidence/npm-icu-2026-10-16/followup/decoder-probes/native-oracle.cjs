const fs=require('fs');
const {internalBinding}=require('internal/test/binding');const icu=internalBinding('icu');
const rows=fs.readFileSync(0,'utf8').trim().split('\n').map(JSON.parse);
for(const r of rows){
 if(r.op==='transcode') {
  const value=icu.transcode(Buffer.from(r.bytes),r.from,r.to);
  console.log(JSON.stringify(typeof value==='number'?{error:icu.icuErrName(value),errno:value}:{bytes:[...value]}));
 }else if(r.op==='width')console.log(JSON.stringify(icu.getStringWidth(String.fromCharCode(...r.units),r.ambiguous,r.expand)));
}
