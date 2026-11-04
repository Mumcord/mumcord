/* Minimal libc header for building Bovnar for wasm32-unknown-unknown (freestanding).
 * The functions are provided by the Mumcord plugin SDK (src/bovnar/libc.rs) or by
 * libc_extra.c in the same prebuilt library. */
#ifndef MC_ERRNO_H
#define MC_ERRNO_H
extern int errno;
#define EINTR 4
#define EBADF 9
#define EAGAIN 11
#define EWOULDBLOCK EAGAIN
#define ENOMEM 12
#define EINVAL 22
#define ENOSPC 28
#define ERANGE 34
#define ENOSYS 38
#endif
