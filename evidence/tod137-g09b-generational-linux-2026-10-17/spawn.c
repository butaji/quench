#include <spawn.h>
#include <stdio.h>
#include <time.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <fcntl.h>
extern char **environ;
/* spawn OUTFILE PROG ARGS... ; prints "maxrss_kib wall_s exit" */
int main(int argc, char **argv) {
  posix_spawn_file_actions_t fa; posix_spawn_file_actions_init(&fa);
  posix_spawn_file_actions_addopen(&fa, 1, argv[1], O_WRONLY|O_CREAT|O_TRUNC, 0644);
  posix_spawn_file_actions_adddup2(&fa, 1, 2);
  struct timespec a,b; clock_gettime(CLOCK_MONOTONIC,&a);
  pid_t pid; if (posix_spawn(&pid, argv[2], &fa, NULL, argv + 2, environ)) { perror("spawn"); return 1; }
  int status; struct rusage ru; wait4(pid, &status, 0, &ru);
  clock_gettime(CLOCK_MONOTONIC,&b);
  printf("%ld %.3f %d\n", ru.ru_maxrss, (b.tv_sec-a.tv_sec)+(b.tv_nsec-a.tv_nsec)/1e9, WEXITSTATUS(status)); return 0;
}
