#include <stdio.h>
#include <math.h>
typedef struct{double x,y,z,vx,vy,vz,m;}B;
void step(B*bs,int n,double dt){for(int i=0;i<n;i++){B*a=&bs[i];for(int j=i+1;j<n;j++){B*b=&bs[j];double dx=a->x-b->x,dy=a->y-b->y,dz=a->z-b->z;double d2=dx*dx+dy*dy+dz*dz+0.01;double mag=dt/(d2*sqrt(d2));a->vx-=dx*b->m*mag;a->vy-=dy*b->m*mag;a->vz-=dz*b->m*mag;b->vx+=dx*a->m*mag;b->vy+=dy*a->m*mag;b->vz+=dz*a->m*mag;}}
for(int i=0;i<n;i++){B*a=&bs[i];a->x+=dt*a->vx;a->y+=dt*a->vy;a->z+=dt*a->vz;}}
int main(){B bs[5];for(int i=0;i<5;i++)bs[i]=(B){i,i*0.5,-i*0.25,0.1*i,0,0.05,1+i*0.1};
for(int s=0;s<400000;s++)step(bs,5,0.001);double e=0;for(int i=0;i<5;i++)e+=bs[i].x+bs[i].y+bs[i].z+bs[i].vx+bs[i].vy+bs[i].vz;printf("%.17g\n",e);}
