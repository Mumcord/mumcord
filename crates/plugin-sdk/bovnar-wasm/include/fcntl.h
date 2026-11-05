/* Minimal libc header for building Bovnar for wasm32-unknown-unknown (freestanding).
 * The functions are provided by the Mumcord plugin SDK (src/bovnar/libc.rs) or by
 * libc_extra.c in the same prebuilt library. */
#ifndef MC_FCNTL_H
#define MC_FCNTL_H
#define O_RDONLY 0
int open(const char *path, int flags, ...);
#endif
