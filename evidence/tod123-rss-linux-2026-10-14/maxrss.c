#include <spawn.h>
#include <stdio.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <fcntl.h>
#include <unistd.h>
extern char **environ;
int main(int argc, char **argv) {
  posix_spawn_file_actions_t fa; posix_spawn_file_actions_init(&fa);
  posix_spawn_file_actions_addopen(&fa, 1, "/dev/null", O_WRONLY, 0);
  pid_t pid; if (posix_spawn(&pid, argv[1], &fa, NULL, argv + 1, environ)) { perror("spawn"); return 1; }
  int status; struct rusage ru; wait4(pid, &status, 0, &ru);
  printf("%ld KiB exit=%d\n", ru.ru_maxrss, WEXITSTATUS(status)); return 0;
}
