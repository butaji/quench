// maxrss CMD...: run CMD, pass its stdout through, then print "maxrss_kb=N wall_ms=N status=N" on stderr.
// Kept tiny and static so the child's inherited RSS high-water (kept across exec) is negligible.
#include <stdio.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
int main(int argc, char **argv) {
  if (argc < 2) return 2;
  struct timespec a, b; clock_gettime(CLOCK_MONOTONIC, &a);
  pid_t pid = fork();
  if (pid == 0) { execvp(argv[1], argv + 1); _exit(127); }
  int status; struct rusage ru; wait4(pid, &status, 0, &ru);
  clock_gettime(CLOCK_MONOTONIC, &b);
  long ms = (b.tv_sec - a.tv_sec) * 1000 + (b.tv_nsec - a.tv_nsec) / 1000000;
  fprintf(stderr, "maxrss_kb=%ld wall_ms=%ld status=%d\n", ru.ru_maxrss, ms, WEXITSTATUS(status));
  return WEXITSTATUS(status);
}
