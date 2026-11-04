/* Minimal libc header for building Bovnar for wasm32-unknown-unknown (freestanding).
 * The functions are provided by the Mumcord plugin SDK (src/bovnar/libc.rs) or by
 * libc_extra.c in the same prebuilt library. */
#ifndef MC_POLL_H
#define MC_POLL_H
#define POLLIN 0x001
#define POLLOUT 0x004
#define POLLERR 0x008
#define POLLNVAL 0x020
struct pollfd { int fd; short events; short revents; };
typedef unsigned long nfds_t;
int poll(struct pollfd *fds, nfds_t n, int timeout);
#endif
