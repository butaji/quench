/* rss_exec CMD...: runs CMD and writes its peak RSS in KiB to the file $RSS_OUT (or stderr), from wait4.
 * A child forked from this small process starts near 1 MB, so the figure is CMD's own peak,
 * not the forking interpreter's (python's ru_maxrss carries over the pre-exec RSS). */
#define _GNU_SOURCE
#include <stdio.h>
#include <sys/resource.h>
#include <sys/wait.h>
#include <stdlib.h>
#include <unistd.h>
int main(int argc, char **argv) {
  if (argc < 2) return 2;
  pid_t pid = fork();
  if (pid == 0) { execvp(argv[1], argv + 1); _exit(127); }
  int st; struct rusage ru;
  wait4(pid, &st, 0, &ru);
  const char *out = getenv("RSS_OUT");
  FILE *f = out ? fopen(out, "w") : NULL;
  fprintf(f ? f : stderr, "%ld\n", ru.ru_maxrss);
  if (f) fclose(f);
  return WIFEXITED(st) ? WEXITSTATUS(st) : 128 + WTERMSIG(st);
}
