/* Minimal libc header for building Bovnar for wasm32-unknown-unknown (freestanding).
 * The functions are provided by the Mumcord plugin SDK (src/bovnar/libc.rs) or by
 * libc_extra.c in the same prebuilt library. */
#ifndef MC_INTTYPES_H
#define MC_INTTYPES_H
#include <stdint.h>
#define PRId32 "d"
#define PRIi32 "i"
#define PRIu32 "u"
#define PRId64 "lld"
#define PRIi64 "lli"
#define PRIu64 "llu"
#endif
