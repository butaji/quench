function last(queue){let p=queue,n;while((n=p.link)!=null)p=n;return p;}
function diamond(p,flag){let n;while(true){n=p.link;if(flag)n=p.other;if(n==null)return p;p=n;}}
let effects=0;
let tail={link:null}, middle={link:tail}, root={link:middle};
let array=[], callable=function(){}, marker={};
for(let i=0;i<128;i++){
    if(last(root)!==tail)throw 'chain';
    tail.link=undefined;if(last(root)!==tail)throw 'undefined';tail.link=null;
    if(last({link:7})!==7)throw 'number link';
    if(last({link:'x'})!=='x'.link)throw 'string link';
    if(last({link:array})!==array)throw 'array link';
    if(last({link:callable})!==callable)throw 'function link';
    let read={get link(){effects++;return root;}};
    if(last(read)!==tail)throw 'getter';
    let thrown={get link(){effects++;throw marker;}};
    try{last(thrown);throw 'missing throw';}catch(error){if(error!==marker)throw error;}
    if(diamond({link:root,other:7},false)!==tail)throw 'diamond object';
    if(diamond({link:root,other:7},true)!==7)throw 'diamond number';
    middle.link=null;if(last(root)!==middle)throw 'changed link';middle.link=tail;
}
if(effects!==256)throw 'duplicate effects';
throw 42;
