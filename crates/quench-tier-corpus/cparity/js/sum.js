function sum(a,n,reps){let s=0;for(let r=0;r<reps;r++){for(let i=0;i<n;i++)s+=a[i];}return s;}
const n=65536,a=new Float64Array(n);for(let i=0;i<n;i++)a[i]=(i%1000)*0.5;
console.log(sum(a,n,600));
