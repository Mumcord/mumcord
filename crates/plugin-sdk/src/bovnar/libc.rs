//! The C library functions the prebuilt Bovnar library calls on `wasm32-unknown-unknown`,
//! where there is no libc. Memory goes through Rust's global allocator. `memcpy`, `memmove`,
//! `memset`, `memcmp`, `strlen` and `ldexp` come from Rust's `compiler_builtins`;
//! the variadic `snprintf`/`printf` live in C inside the prebuilt library (`bovnar-wasm/libc_extra.c`)
//! because stable Rust cannot define C-variadic functions.
//!
//! Every function follows its C standard contract; the callers are the Bovnar C sources only.
//! The symbols are exported on wasm32 only (natively the system libc serves the library); the
//! functions still compile natively so the unit tests below can check them.

// Natively nothing outside the tests calls these.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::alloc::{Layout, alloc, dealloc, realloc as rust_realloc};
use std::ffi::{c_char, c_int, c_long, c_longlong, c_ulonglong, c_void};
use std::ptr;

/// `malloc` must return memory aligned for any C type; 16 also covers `long double`.
const ALIGN: usize = 16;
/// Each block starts with a header holding its size, because `free` gets no size and Rust's
/// allocator needs one. The header is a full `ALIGN` so the payload stays aligned.
const HEADER: usize = ALIGN;

const EBADF: c_int = 9;
const EINVAL: c_int = 22;
const ERANGE: c_int = 34;

/// The C `errno`. A plugin is single-threaded, so a plain global is what C expects.
#[allow(non_upper_case_globals)] // the C name
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub static mut errno: c_int = 0;

fn set_errno(value: c_int) {
    // SAFETY: plugins run on one thread; nothing holds a reference to `errno`.
    unsafe { ptr::addr_of_mut!(errno).write(value) };
}

fn layout(size: usize) -> Option<Layout> {
    Layout::from_size_align(size.checked_add(HEADER)?, ALIGN).ok()
}

/// # Safety
/// C `malloc`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    let Some(layout) = layout(size) else {
        return ptr::null_mut();
    };
    // SAFETY: the layout is never zero-sized (it includes the header).
    let base = unsafe { alloc(layout) };
    if base.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: `base` holds HEADER bytes, aligned for usize, before the payload.
    unsafe {
        base.cast::<usize>().write(size);
        base.add(HEADER).cast()
    }
}

/// # Safety
/// C `calloc`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn calloc(count: usize, size: usize) -> *mut c_void {
    let Some(total) = count.checked_mul(size) else {
        return ptr::null_mut();
    };
    // SAFETY: C contract of malloc.
    let p = unsafe { malloc(total) };
    if !p.is_null() {
        // SAFETY: `p` holds `total` bytes.
        unsafe { ptr::write_bytes(p.cast::<u8>(), 0, total) };
    }
    p
}

/// # Safety
/// C `free`: `p` is null or came from `malloc`/`calloc`/`realloc` and was not freed yet.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn free(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    // SAFETY: the header sits HEADER bytes before every block this shim hands out.
    unsafe {
        let base = p.cast::<u8>().sub(HEADER);
        let size = base.cast::<usize>().read();
        dealloc(
            base,
            Layout::from_size_align_unchecked(size + HEADER, ALIGN),
        );
    }
}

/// # Safety
/// C `realloc`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn realloc(p: *mut c_void, size: usize) -> *mut c_void {
    if p.is_null() {
        // SAFETY: C contract of malloc.
        return unsafe { malloc(size) };
    }
    let Some(new_layout) = layout(size) else {
        return ptr::null_mut();
    };
    // SAFETY: `p` came from this shim, so its header holds the old size.
    unsafe {
        let base = p.cast::<u8>().sub(HEADER);
        let old = base.cast::<usize>().read();
        let old_layout = Layout::from_size_align_unchecked(old + HEADER, ALIGN);
        let grown = rust_realloc(base, old_layout, new_layout.size());
        if grown.is_null() {
            return ptr::null_mut();
        }
        grown.cast::<usize>().write(size);
        grown.add(HEADER).cast()
    }
}

/// # Safety
/// C `memchr`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn memchr(s: *const c_void, c: c_int, n: usize) -> *mut c_void {
    // SAFETY: the caller guarantees `n` readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(s.cast::<u8>(), n) };
    match bytes.iter().position(|&b| b == c as u8) {
        // SAFETY: `i < n`.
        Some(i) => unsafe { s.cast::<u8>().add(i).cast_mut().cast() },
        None => ptr::null_mut(),
    }
}

/// # Safety
/// C `strcmp`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn strcmp(a: *const c_char, b: *const c_char) -> c_int {
    let mut i = 0;
    loop {
        // SAFETY: both strings are NUL-terminated; we stop at the first NUL.
        let (x, y) = unsafe { (*a.add(i) as u8, *b.add(i) as u8) };
        if x != y || x == 0 {
            return c_int::from(x) - c_int::from(y);
        }
        i += 1;
    }
}

/// # Safety
/// C `strncpy`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn strncpy(dst: *mut c_char, src: *const c_char, n: usize) -> *mut c_char {
    let mut i = 0;
    // SAFETY: `dst` holds `n` bytes; `src` is NUL-terminated or at least `n` long.
    unsafe {
        while i < n && *src.add(i) != 0 {
            *dst.add(i) = *src.add(i);
            i += 1;
        }
        ptr::write_bytes(dst.add(i), 0, n - i);
    }
    dst
}

/// The digits of a C integer literal after `strtoull`'s prefix rules: (start of the digits,
/// negative, base). None for an invalid base.
///
/// # Safety
/// `s` is NUL-terminated.
unsafe fn number_start(s: *const c_char, base: c_int) -> Option<(*const c_char, bool, u32)> {
    if base != 0 && !(2..=36).contains(&base) {
        return None;
    }
    let mut p = s;
    // SAFETY: we never read past the terminating NUL.
    unsafe {
        while matches!(*p as u8, b' ' | b'\t'..=b'\r') {
            p = p.add(1);
        }
        let negative = *p as u8 == b'-';
        if matches!(*p as u8, b'+' | b'-') {
            p = p.add(1);
        }
        let hex_prefix = *p as u8 == b'0' && matches!(*p.add(1) as u8, b'x' | b'X');
        let mut base = base as u32;
        if (base == 0 || base == 16) && hex_prefix && (*p.add(2) as u8).is_ascii_hexdigit() {
            p = p.add(2);
            base = 16;
        } else if base == 0 {
            base = if *p as u8 == b'0' { 8 } else { 10 };
        }
        Some((p, negative, base))
    }
}

/// Parses digits in `base` from `p`: (magnitude, overflowed, end). `end == p` means no digits.
///
/// # Safety
/// `p` is NUL-terminated.
unsafe fn digits(p: *const c_char, base: u32) -> (u64, bool, *const c_char) {
    let (mut value, mut overflow, mut q) = (0u64, false, p);
    // SAFETY: we stop at the first non-digit, at the latest the NUL.
    while let Some(d) = (unsafe { *q } as u8 as char).to_digit(base) {
        match value
            .checked_mul(u64::from(base))
            .and_then(|v| v.checked_add(u64::from(d)))
        {
            Some(v) => value = v,
            None => overflow = true,
        }
        // SAFETY: the current byte is a digit, not the NUL.
        q = unsafe { q.add(1) };
    }
    (value, overflow, q)
}

/// # Safety
/// C `strtoull`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn strtoull(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_ulonglong {
    // SAFETY: C contract: `s` is NUL-terminated, `end` is null or writable.
    unsafe {
        let Some((p, negative, base)) = number_start(s, base) else {
            set_errno(EINVAL);
            return 0;
        };
        let (value, overflow, q) = digits(p, base);
        if !end.is_null() {
            *end = if q == p { s.cast_mut() } else { q.cast_mut() };
        }
        if overflow {
            set_errno(ERANGE);
            return u64::MAX;
        }
        if negative {
            value.wrapping_neg()
        } else {
            value
        }
    }
}

/// # Safety
/// C `strtoll`.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn strtoll(
    s: *const c_char,
    end: *mut *mut c_char,
    base: c_int,
) -> c_longlong {
    // SAFETY: C contract: `s` is NUL-terminated, `end` is null or writable.
    unsafe {
        let Some((p, negative, base)) = number_start(s, base) else {
            set_errno(EINVAL);
            return 0;
        };
        let (value, overflow, q) = digits(p, base);
        if !end.is_null() {
            *end = if q == p { s.cast_mut() } else { q.cast_mut() };
        }
        let limit = if negative {
            i64::MIN.unsigned_abs()
        } else {
            i64::MAX as u64
        };
        if overflow || value > limit {
            set_errno(ERANGE);
            return if negative { i64::MIN } else { i64::MAX };
        }
        if negative {
            (value as i64).wrapping_neg()
        } else {
            value as i64
        }
    }
}

/// # Safety
/// C `qsort`. A heap sort: no allocation, O(n log n) on any input.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn qsort(
    base: *mut c_void,
    n: usize,
    size: usize,
    cmp: unsafe extern "C" fn(*const c_void, *const c_void) -> c_int,
) {
    let base = base.cast::<u8>();
    let at = |i: usize| base.wrapping_add(i * size);
    // SAFETY: the caller guarantees `n` elements of `size` bytes; indices stay below `n`.
    let less = |i: usize, j: usize| unsafe { cmp(at(i).cast(), at(j).cast()) < 0 };
    // SAFETY: as above; `i != j` whenever this is called.
    let swap = |i: usize, j: usize| unsafe { ptr::swap_nonoverlapping(at(i), at(j), size) };
    let sift_down = |mut root: usize, len: usize| loop {
        let mut child = 2 * root + 1;
        if child >= len {
            break;
        }
        if child + 1 < len && less(child, child + 1) {
            child += 1;
        }
        if !less(root, child) {
            break;
        }
        swap(root, child);
        root = child;
    };
    for start in (0..n / 2).rev() {
        sift_down(start, n);
    }
    for end in (1..n).rev() {
        swap(0, end);
        sift_down(0, end);
    }
}

/// # Safety
/// C `frexp`: `exp` is writable.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub unsafe extern "C" fn frexp(x: f64, exp: *mut c_int) -> f64 {
    let bits = x.to_bits();
    let biased = ((bits >> 52) & 0x7ff) as c_int;
    let (mantissa, e) = if x == 0.0 || !x.is_finite() {
        (x, 0)
    } else if biased == 0 {
        // Subnormal: scale into the normal range first.
        let mut e = 0;
        // SAFETY: `e` is a local.
        let m = unsafe { frexp(x * f64::from_bits(0x43f0_0000_0000_0000), &mut e) }; // 2^64
        (m, e - 64)
    } else {
        // Keep sign and fraction, set the exponent so the result lies in [0.5, 1).
        (
            f64::from_bits((bits & !(0x7ff << 52)) | (1022 << 52)),
            biased - 1022,
        )
    };
    // SAFETY: the caller guarantees `exp` is writable.
    unsafe { exp.write(e) };
    mantissa
}

/// A plugin has no file descriptors: the library's fd sources and sinks always fail.
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn read(_fd: c_int, _buf: *mut c_void, _n: usize) -> c_long {
    set_errno(EBADF);
    -1
}

/// See [`read`].
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn write(_fd: c_int, _buf: *const c_void, _n: usize) -> c_long {
    set_errno(EBADF);
    -1
}

/// See [`read`].
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn poll(_fds: *mut c_void, _n: usize, _timeout: c_int) -> c_int {
    set_errno(EBADF);
    -1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn parse_ll(text: &str, base: c_int) -> (i64, usize) {
        let s = CString::new(text).expect("no NUL");
        let mut end = ptr::null_mut();
        // SAFETY: `s` is NUL-terminated and `end` is writable.
        let v = unsafe { strtoll(s.as_ptr(), &mut end, base) };
        (v, end as usize - s.as_ptr() as usize)
    }

    #[test]
    fn strtoll_follows_c() {
        assert_eq!(parse_ll("  -42x", 10), (-42, 5));
        assert_eq!(parse_ll("0x1f", 0), (31, 4));
        assert_eq!(parse_ll("ff", 16), (255, 2));
        assert_eq!(parse_ll("x", 10), (0, 0));
        assert_eq!(parse_ll("9223372036854775808", 10).0, i64::MAX);
        assert_eq!(parse_ll("-9223372036854775808", 10).0, i64::MIN);
    }

    #[test]
    fn strtoull_saturates() {
        let s = CString::new("18446744073709551616").expect("no NUL");
        // SAFETY: `s` is NUL-terminated; `end` may be null.
        assert_eq!(
            unsafe { strtoull(s.as_ptr(), ptr::null_mut(), 10) },
            u64::MAX
        );
    }

    unsafe extern "C" fn cmp_i32(a: *const c_void, b: *const c_void) -> c_int {
        // SAFETY: qsort passes pointers to i32 elements.
        let (a, b) = unsafe { (*a.cast::<i32>(), *b.cast::<i32>()) };
        a.cmp(&b) as c_int
    }

    #[test]
    fn qsort_sorts() {
        let mut v = [5, -1, 3, 3, 0, 9, 2];
        // SAFETY: `v` holds 7 i32 elements.
        unsafe { qsort(v.as_mut_ptr().cast(), v.len(), 4, cmp_i32) };
        assert_eq!(v, [-1, 0, 2, 3, 3, 5, 9]);
    }

    #[test]
    fn allocator_round_trips() {
        // SAFETY: C allocator contract, each block freed once.
        unsafe {
            let p = calloc(4, 8).cast::<u8>();
            assert!(!p.is_null() && (p as usize).is_multiple_of(ALIGN));
            assert!((0..32).all(|i| *p.add(i) == 0));
            *p.add(31) = 7;
            let q = realloc(p.cast(), 4096).cast::<u8>();
            assert_eq!(*q.add(31), 7);
            free(q.cast());
            free(ptr::null_mut());
        }
    }

    #[test]
    fn frexp_splits() {
        let split = |x: f64| {
            let mut e = 0;
            // SAFETY: `e` is a local.
            (unsafe { frexp(x, &mut e) }, e)
        };
        assert_eq!(split(8.0), (0.5, 4));
        assert_eq!(split(-3.0), (-0.75, 2));
        assert_eq!(split(0.0), (0.0, 0));
        assert_eq!(split(f64::MIN_POSITIVE / 4.0), (0.5, -1023));
        assert!(split(f64::INFINITY).0.is_infinite());
    }

    #[test]
    fn strings() {
        let (a, b) = (
            CString::new("abc").expect("ok"),
            CString::new("abd").expect("ok"),
        );
        // SAFETY: NUL-terminated strings, an 8-byte buffer.
        unsafe {
            assert!(strcmp(a.as_ptr(), b.as_ptr()) < 0);
            assert_eq!(strcmp(a.as_ptr(), a.as_ptr()), 0);
            let mut buf = [1 as c_char; 8];
            strncpy(buf.as_mut_ptr(), a.as_ptr(), 8);
            assert_eq!(&buf[..4], &[97, 98, 99, 0]);
            assert_eq!(buf[7], 0);
            assert_eq!(
                memchr(a.as_ptr().cast(), b'c' as c_int, 3) as usize,
                a.as_ptr() as usize + 2
            );
        }
    }
}
