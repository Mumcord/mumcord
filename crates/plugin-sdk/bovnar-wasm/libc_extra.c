/* The variadic libc functions the Bovnar C code calls. They live in C because
 * stable Rust cannot define C-variadic functions; everything else the library
 * needs comes from the SDK's Rust shim (src/bovnar/libc.rs).
 *
 * Only the conversions Bovnar uses are supported: %d %i %u with the l/ll
 * modifiers, %s, %c and %%, plus the '0' and '-' flags, a width and an
 * integer precision. */
#include <stdarg.h>
#include <stddef.h>
#include <stdio.h>

struct out { char *buf; size_t size; size_t len; };

static void put(struct out *o, char c)
{
	if (o->len + 1 < o->size) o->buf[o->len] = c;
	o->len++;
}

static void put_padded(struct out *o, const char *s, size_t n, int width, int left, char pad)
{
	int fill = width > (int)n ? width - (int)n : 0;
	if (!left)
		for (; fill > 0; fill--) put(o, pad);
	for (size_t i = 0; i < n; i++) put(o, s[i]);
	for (; fill > 0; fill--) put(o, ' ');
}

int vsnprintf(char *buf, size_t size, const char *fmt, va_list ap)
{
	struct out o = { buf, size, 0 };
	for (; *fmt; fmt++) {
		if (*fmt != '%') { put(&o, *fmt); continue; }
		fmt++;
		int left = 0, zero = 0, width = 0, prec = -1, lng = 0;
		for (;; fmt++) {
			if (*fmt == '-') left = 1;
			else if (*fmt == '0') zero = 1;
			else break;
		}
		while (*fmt >= '0' && *fmt <= '9') width = width * 10 + (*fmt++ - '0');
		if (*fmt == '.') {
			prec = 0;
			fmt++;
			while (*fmt >= '0' && *fmt <= '9') prec = prec * 10 + (*fmt++ - '0');
		}
		while (*fmt == 'l') { lng++; fmt++; }
		char tmp[48];
		switch (*fmt) {
		case 'd': case 'i': case 'u': {
			unsigned long long v;
			int neg = 0;
			if (*fmt == 'u')
				v = lng >= 2 ? va_arg(ap, unsigned long long)
				  : lng == 1 ? va_arg(ap, unsigned long) : va_arg(ap, unsigned int);
			else {
				long long s = lng >= 2 ? va_arg(ap, long long)
					    : lng == 1 ? va_arg(ap, long) : va_arg(ap, int);
				neg = s < 0;
				v = neg ? 0ull - (unsigned long long)s : (unsigned long long)s;
			}
			char digits[24];
			int nd = 0;
			do { digits[nd++] = (char)('0' + v % 10); v /= 10; } while (v);
			size_t n = 0;
			if (neg) tmp[n++] = '-';
			for (int p = nd; p < prec; p++) tmp[n++] = '0';
			while (nd) tmp[n++] = digits[--nd];
			if (zero && !left && prec < 0 && neg && width > (int)n) {
				/* the sign goes before the zero padding */
				put(&o, '-');
				put_padded(&o, tmp + 1, n - 1, width - 1, 0, '0');
			} else {
				put_padded(&o, tmp, n, width, left, zero && prec < 0 ? '0' : ' ');
			}
			break;
		}
		case 's': {
			const char *s = va_arg(ap, const char *);
			size_t n = 0;
			while (s[n] && (prec < 0 || n < (size_t)prec)) n++;
			put_padded(&o, s, n, width, left, ' ');
			break;
		}
		case 'c':
			tmp[0] = (char)va_arg(ap, int);
			put_padded(&o, tmp, 1, width, left, ' ');
			break;
		case '%':
			put(&o, '%');
			break;
		default:
			/* An unsupported conversion is a bug in this shim, not in the input. */
			return -1;
		}
	}
	if (size) o.buf[o.len < size ? o.len : size - 1] = '\0';
	return (int)o.len;
}

int snprintf(char *buf, size_t size, const char *fmt, ...)
{
	va_list ap;
	va_start(ap, fmt);
	int n = vsnprintf(buf, size, fmt, ap);
	va_end(ap);
	return n;
}

/* Only bvn_datetime.c's debug dump prints; a plugin has no stdout. */
int printf(const char *fmt, ...)
{
	(void)fmt;
	return 0;
}
