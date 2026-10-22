const fixtures=[
 ['utf-8',[0xef,0xbb,0xbf,0x41],[0xe2,0x28,0xa1]],
 ['utf-16le',[0xff,0xfe,0x41,0],[0,0xd8,0x41,0]],
 ['utf-16be',[0xfe,0xff,0,0x41],[0xd8,0,0,0x41]],
 ['shift_jis',[0x82,0xa0,0x41],[0x82,0x20,0xff]],
 ['big5',[0xa4,0xa4,0x41],[0xa4,0x20,0xff]],
 ['euc-kr',[0xb0,0xa1,0x41],[0xb0,0x20,0xff]],
 ['gb18030',[0x90,0x30,0x81,0x30,0x41],[0x90,0x30,0x20,0xff]],
 ['iso-2022-jp',[27,36,66,36,34,27,40,66,65],[27,36,255,27]],
 ['windows-1252',[0x41,0x80],[0x81,0xff]],
];
const rows=[];
for(const [encoding,valid,invalid]of fixtures)for(const fatal of [false,true])for(const ignoreBOM of [false,true])for(let cut=0;cut<=invalid.length;cut++){
 const decoder=new TextDecoder(encoding,{fatal,ignoreBOM}),out=[];
 const steps=[[invalid.slice(0,cut),true],[invalid.slice(cut),true],[[],false],[valid.slice(0,1),true],[valid.slice(1),false],[[],false],[valid,false]];
 for(const[bytes,stream]of steps)try{out.push({value:decoder.decode(Uint8Array.from(bytes),{stream})});}catch(e){out.push({error:{name:e.name,code:e.code??null,message:e.message}});}
 rows.push([encoding,fatal,ignoreBOM,cut,out]);
}
console.log(JSON.stringify({complete:true,cases:rows.length,calls:rows.length*7,rows}));
