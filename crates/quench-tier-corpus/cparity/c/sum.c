#include <stdio.h>
#include <stdlib.h>
double sum(double*a,int n,int reps){double s=0;for(int r=0;r<reps;r++){for(int i=0;i<n;i++)s+=a[i];}return s;}
int main(){int n=65536;double*a=malloc(n*8);for(int i=0;i<n;i++)a[i]=(i%1000)*0.5;printf("%.17g\n",sum(a,n,600));}
