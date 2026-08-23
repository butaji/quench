const c=require('crypto'); const x=new Uint8Array([113,117,101,110]); const r=c.timingSafeEqual(x,x); console.log(typeof r, r===true, String(r));
