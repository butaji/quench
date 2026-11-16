#include <stdio.h>
void mm(double*A,double*B,double*C,int n){for(int i=0;i<n;i++)for(int j=0;j<n;j++){double s=0;for(int k=0;k<n;k++)s+=A[i*n+k]*B[k*n+j];C[i*n+j]=s;}}
int main(){enum{n=128};static double A[n*n],B[n*n],C[n*n];for(int i=0;i<n*n;i++){A[i]=(i%13)*0.5;B[i]=(i%7)-3;}
for(int r=0;r<10;r++)mm(A,B,C,n);double t=0;for(int i=0;i<n*n;i++)t+=C[i];printf("%.17g\n",t);}
