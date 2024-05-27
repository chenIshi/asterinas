#include <unistd.h>
#include <stdio.h>

int main(void)
{
  fork();

  char *argv[] = { "/regression/network/speedtest", NULL };
  char *envp[] = { "home=/", "version=1.1", NULL };
  execve("/regression/network/speedtest", argv, envp);

  perror("execve");
}
