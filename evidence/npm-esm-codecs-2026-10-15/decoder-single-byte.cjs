const labels=['utf-8','utf-16le','utf-16be','windows-874','windows-1250','windows-1251','windows-1252','windows-1253','windows-1254','windows-1255','windows-1256','windows-1257','windows-1258','iso-8859-2','iso-8859-3','iso-8859-4','iso-8859-5','iso-8859-6','iso-8859-7','iso-8859-8','iso-8859-8-i','iso-8859-10','iso-8859-13','iso-8859-14','iso-8859-15','iso-8859-16','koi8-r','koi8-u','ibm866','macintosh','x-mac-cyrillic','gbk','gb18030','big5','shift_jis','euc-jp','euc-kr','iso-2022-jp','x-user-defined'];
const out=[];
for(const label of labels)for(const fatal of [false,true]){
 const decoder=new TextDecoder(label,{fatal});const values=[];
 for(let b=0;b<256;b++)try{values.push(decoder.decode(new Uint8Array([b])))}catch(e){values.push([e.name,e.code,e.message]);}
 out.push([label,fatal,decoder.encoding,values]);
}
console.log(JSON.stringify(out));
