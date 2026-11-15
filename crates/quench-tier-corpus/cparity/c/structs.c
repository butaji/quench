#include <stdio.h>
typedef struct{double x,y,vx,vy;int k;}P;
void upd(P*ps,int n,int reps){for(int r=0;r<reps;r++){for(int i=0;i<n;i++){P*p=&ps[i];p->x+=p->vx;p->y+=p->vy;if(p->x>100)p->vx=-p->vx;if(p->y>100)p->vy=-p->vy;p->k=p->k+1;}}}
int main(){enum{n=1024};static P ps[n];for(int i=0;i<n;i++)ps[i]=(P){i%50,i%70,0.5,0.25,0};upd(ps,n,2000);double t=0;for(int i=0;i<n;i++)t+=ps[i].x+ps[i].y+ps[i].k;printf("%.17g\n",t);}
