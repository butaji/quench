const inputs=[['utf-8',[239,187,191,226,130,172,240,159,152,128,65]],['utf-16le',[255,254,61,216,0,222,65,0]],['utf-16be',[254,255,216,61,222,0,0,65]],['shift_jis',[130,160,65]],['big5',[164,164,65]],['euc-kr',[176,161,65]],['gb18030',[144,48,129,48,65]],['iso-2022-jp',[27,36,66,36,34,27,40,66,65]]];
const rows=[];
for(const [encoding,bytes]of inputs)for(const fatal of [false,true])for(const ignoreBOM of [false,true])for(let cut=0;cut<=bytes.length;cut++){
 const decoder=new TextDecoder(encoding,{fatal,ignoreBOM});
 try {const first=decoder.decode(Uint8Array.from(bytes.slice(0,cut)),{stream:true});const second=decoder.decode(Uint8Array.from(bytes.slice(cut)));const flushed=decoder.decode();rows.push([encoding,fatal,ignoreBOM,cut,[first,second,flushed]]);}
 catch(e){rows.push([encoding,fatal,ignoreBOM,cut,{name:e.name,code:e.code??null,message:e.message}]);}
}
console.log(JSON.stringify({complete:true,cases:rows.length,rows}));
