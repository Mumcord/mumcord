/* Minimal libc header for building Bovnar for wasm32-unknown-unknown (freestanding).
 * The functions are provided by the Mumcord plugin SDK (src/bovnar/libc.rs) or by
 * libc_extra.c in the same prebuilt library. */
#ifndef MC_STDLIB_H
#define MC_STDLIB_H
#include <stddef.h>
void *malloc(size_t size);
void *calloc(size_t n, size_t size);
void *realloc(void *ptr, size_t size);
void free(void *ptr);
_Noreturn void abort(void);
_Noreturn void exit(int status);
long long strtoll(const char *s, char **end, int base);
unsigned long long strtoull(const char *s, char **end, int base);
void qsort(void *base, size_t n, size_t size, int (*cmp)(const void *, const void *));
#endif
