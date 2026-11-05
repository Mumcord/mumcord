/* Minimal libc header for building Bovnar for wasm32-unknown-unknown (freestanding).
 * The functions are provided by the Mumcord plugin SDK (src/bovnar/libc.rs) or by
 * libc_extra.c in the same prebuilt library. */
#ifndef MC_MATH_H
#define MC_MATH_H
#define INFINITY (__builtin_inff())
#define NAN (__builtin_nanf(""))
#define HUGE_VAL (__builtin_huge_val())
#define isnan(x) __builtin_isnan(x)
#define isinf(x) __builtin_isinf(x)
#define isfinite(x) __builtin_isfinite(x)
#define signbit(x) __builtin_signbit(x)
/* These lower to single wasm instructions. */
static inline double fabs(double x) { return __builtin_fabs(x); }
static inline double floor(double x) { return __builtin_floor(x); }
static inline double ceil(double x) { return __builtin_ceil(x); }
double round(double x);
double pow(double x, double y);
double log10(double x);
double log2(double x);
double ldexp(double x, int e);
double frexp(double x, int *e);
#endif
