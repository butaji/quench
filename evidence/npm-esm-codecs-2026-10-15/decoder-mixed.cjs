const labels=['utf-8','utf8','unicode-1-1-utf-8','utf-16','utf-16le','utf-16be','unicodefffe','ascii','latin1','windows-1252','iso-8859-2','koi8-r','gbk','gb18030','big5','shift_jis','euc-jp','iso-2022-jp','euc-kr','x-user-defined','replacement','iso-2022-cn','bogus','\u00a0utf-8\u00a0'];
const vectors=[[],[65],[239,187,191,65],[255,254,65,0],[254,255,0,65],[0,216],[0,216,65],[128],[255],[195,40],[136,98]];
const out=[];
for(const label of labels)for(const fatal of [false,true])for(const ignoreBOM of [false,true]){
 let decoder;try{decoder=new TextDecoder(label,{fatal,ignoreBOM})}catch(e){out.push([label,fatal,ignoreBOM,e.name,e.code,e.message]);continue}
 for(const bytes of vectors)try{out.push([label,fatal,ignoreBOM,bytes,decoder.encoding,decoder.decode(new Uint8Array(bytes))])}catch(e){out.push([label,fatal,ignoreBOM,bytes,e.name,e.code,e.message])}
}
console.log(JSON.stringify(out));
