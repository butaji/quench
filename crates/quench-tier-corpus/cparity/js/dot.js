function dot(a,b,n,reps){let s=0;for(let r=0;r<reps;r++){for(let i=0;i<n;i++)s+=a[i]*b[i];}return s;}
const n=32768,a=new Float64Array(n),b=new Float64Array(n);for(let i=0;i<n;i++){a[i]=(i%100)*0.25;b[i]=(i%7)+1;}
console.log(dot(a,b,n,600));
