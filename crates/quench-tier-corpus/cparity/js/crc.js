function crc(b,n,reps){let c=-1|0,x=123456789|0;for(let r=0;r<reps;r++){for(let i=0;i<n;i++){x^=x<<13;x^=x>>>17;x^=x<<5;c=(c>>>8)^((c^b[i]^x)&255)*2654435761;c|=0;}}return c|0;}
const n=65536,b=new Uint8Array(n);for(let i=0;i<n;i++)b[i]=(i*31+7)&255;
console.log(crc(b,n,60));
