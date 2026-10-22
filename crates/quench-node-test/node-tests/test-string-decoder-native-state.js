const {StringDecoder}=require(process.argv[2]||'string_decoder');
const units=s=>Array.from({length:s.length},(_,i)=>s.charCodeAt(i));
const inputs=[[],[97],[0xc0],[0xc1],[0xe0,0x80],[0xed,0xa0,0x80],[0xf0,0x80,0x80],[0xf5,0x80],[0xf8],[0xff],[0xe2,0x82,0xac],[0xf0,0x9f,0x98,0x80],[0x3d,0xd8,0,0xde],[0x61,0,0x3d,0xd8,0],[0x80,0x81,0x82,0x83,0x84],[1,2,3,4,5,6,7]];
for(const encoding of ['utf8','utf16le','ascii','latin1','base64','base64url','hex'])for(const bytes of inputs)for(let split=0;split<=bytes.length;split++){
 const decoder=new StringDecoder(encoding),chunks=[bytes.slice(0,split),bytes.slice(split)],rows=[];
 for(const chunk of chunks){const value=decoder.write(Buffer.from(chunk));rows.push([units(value),decoder.lastNeed,decoder.lastTotal,[...decoder.lastChar]])}
 rows.push([units(decoder.end()),decoder.lastNeed,decoder.lastTotal,[...decoder.lastChar]]);
 console.log(JSON.stringify([encoding,bytes,split,rows]));
}
for(const encoding of [undefined,null,false,0,NaN,'','UTF-8','ucs-2','bad']){
 try{const d=new StringDecoder(encoding);console.log(JSON.stringify(['constructor',String(encoding),d.encoding,units(d.write('hé😀')),d.lastNeed,d.lastTotal]))}catch(e){console.log(JSON.stringify(['constructor',String(encoding),e.name,e.code,e.message]))}
}
console.log(JSON.stringify(['methods',...['write','end','text'].map(k=>[k,StringDecoder.prototype[k].name,StringDecoder.prototype[k].length])]));
