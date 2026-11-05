/* Minimal libc header for building Bovnar for wasm32-unknown-unknown (freestanding).
 * The functions are provided by the Mumcord plugin SDK (src/bovnar/libc.rs) or by
 * libc_extra.c in the same prebuilt library. */
#ifndef MC_ASSERT_H
#define MC_ASSERT_H
#ifdef NDEBUG
#  define assert(x) ((void)0)
#else
void abort(void);
#  define assert(x) ((x) ? (void)0 : abort())
#endif
#endif
