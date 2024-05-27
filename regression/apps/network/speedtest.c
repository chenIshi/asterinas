#include <stdio.h>
#include <time.h>
#include <unistd.h>
#include <sys/socket.h>
#include <arpa/inet.h>

#define TRANSFER_SIZE (1024 * 1024 * 1024)

static char buf[4096] = "Hello, world!";

static inline long milliseconds(void)
{
  struct timespec tp;

  if (clock_gettime(CLOCK_MONOTONIC, &tp) < 0) {
    return 0;
  }

  return tp.tv_sec * 1000 + tp.tv_nsec / (1000 * 1000);
}

int start_server(void)
{
  const int on = 1;
  int sockfd, client_fd;
  struct sockaddr_in server_addr;
  struct sockaddr client_addr;
  socklen_t client_addrlen = sizeof(client_addr);
  ssize_t bytes = TRANSFER_SIZE;
  long time;

  sockfd = socket(AF_INET, SOCK_STREAM, 0);
  if (sockfd < 0) {
    perror("socket");
    return -1;
  }

  server_addr.sin_family = AF_INET;
  server_addr.sin_port = htons(8080);
  if (inet_pton(AF_INET, "127.0.0.1", &server_addr.sin_addr) != 1) {
    perror("inet_pton");
    return -1;
  }

  if (setsockopt(sockfd, SOL_SOCKET, SO_REUSEADDR, &on, sizeof(on)) != 0) {
    perror("setsockopt");
    return -1;
  }

  if (bind(sockfd, (struct sockaddr *)&server_addr, sizeof(server_addr)) != 0) {
    perror("bind");
    return -1;
  }

  if (listen(sockfd, 1) != 0) {
    perror("listen");
    return -1;
  }

  client_fd = accept(sockfd, &client_addr, &client_addrlen);
  if (client_fd < 0) {
    perror("accept");
    return -1;
  }

  time = milliseconds();

  while (bytes > 0)
  {
    ssize_t sz = bytes > sizeof(buf) ? sizeof(buf) : bytes;

    sz = send(client_fd, buf, sz, 0);
    if (sz < 0) {
      perror("send");
      return -1;
    }

    bytes -= (sz >> 0);
  }

  time = milliseconds() - time;
  printf("[send] %.2lf seconds, %.2lf Gbps\n",
         time / 1000.0,
         TRANSFER_SIZE * 8.0 / time / 1000 / 1000);

  close(client_fd);
  close(sockfd);

  return 0;
}

int start_client(void)
{
  int sockfd;
  struct sockaddr_in server_addr;
  ssize_t bytes = TRANSFER_SIZE;
  long time;

  sockfd = socket(AF_INET, SOCK_STREAM, 0);
  if (sockfd < 0) {
    perror("socket");
    return -1;
  }

  server_addr.sin_family = AF_INET;
  server_addr.sin_port = htons(8080);
  if (inet_pton(AF_INET, "127.0.0.1", &server_addr.sin_addr) != 1) {
    perror("inet_pton");
    return -1;
  }

  if (connect(sockfd, (struct sockaddr *)&server_addr, sizeof(server_addr)) != 0) {
    perror("connect");
    return -1;
  }

  time = milliseconds();

  while (bytes > 0)
  {
    ssize_t sz = recv(sockfd, buf, sizeof(buf), 0);
    if (sz < 0) {
      perror("recv");
      return -1;
    }

    /*
    if ((bytes >> 20) != ((bytes - sz) >> 20))
      printf("%d MiB remaining...\n", (int)(bytes >> 20));
    */
    bytes -= (sz >> 0);
  }

  time = milliseconds() - time;
  printf("[recv] %.2lf seconds, %.2lf Gbps\n",
         time / 1000.0,
         TRANSFER_SIZE * 8.0 / time / 1000 / 1000);

  close(sockfd);

  return 0;
}

int main(void)
{
  // FIXME: Fix COW bugs

  // int pid = fork();

  // if (pid < 0) {
  //   perror("fork");
  //   return -1;
  // }

  start_server();
  start_client();

  return 0;
}

