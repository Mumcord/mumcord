//! The Mumble Link shared memory segment, opened or created the way the official client does:
//! POSIX shared memory `/MumbleLink.<uid>` on Linux and macOS, the named file mapping
//! `MumbleLink` on Windows. Games attach to the same segment and write into it.

use std::io;

/// A mapped segment of at least `len` bytes, readable and writable.
pub(crate) struct SharedMemory {
    ptr: *mut u8,
    len: usize,
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// The mapping is plain memory owned by this value; access goes through `&self`/`&mut self`.
unsafe impl Send for SharedMemory {}

impl SharedMemory {
    /// Copies the segment into `out` (resized to the segment length). The game writes
    /// concurrently; torn reads are tolerated like in the official client (the tick tells
    /// whether anything changed, fields are validated when parsed).
    pub(crate) fn read_into(&self, out: &mut Vec<u8>) {
        out.resize(self.len, 0);
        // SAFETY: `ptr` maps `len` bytes for the lifetime of `self`; volatile because another
        // process changes the memory.
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = unsafe { self.ptr.add(i).read_volatile() };
        }
    }

    /// Reads the little 32-bit word at `offset` (native byte order).
    pub(crate) fn read_u32(&self, offset: usize) -> u32 {
        assert!(offset + 4 <= self.len);
        let mut bytes = [0u8; 4];
        for (i, byte) in bytes.iter_mut().enumerate() {
            // SAFETY: bounds checked above; see `read_into`.
            *byte = unsafe { self.ptr.add(offset + i).read_volatile() };
        }
        u32::from_ne_bytes(bytes)
    }

    #[cfg(all(test, unix))]
    pub(crate) fn write(&mut self, data: &[u8]) {
        assert!(data.len() <= self.len);
        // SAFETY: bounds checked above.
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), self.ptr, data.len()) };
    }
}

#[cfg(unix)]
impl SharedMemory {
    /// The segment name of the current user (`/MumbleLink.<uid>`).
    pub(crate) fn default_name() -> String {
        // SAFETY: getuid has no preconditions and cannot fail.
        format!("/MumbleLink.{}", unsafe { libc::getuid() })
    }

    pub(crate) fn open(name: &str, len: usize) -> io::Result<Self> {
        use std::ffi::CString;
        let cname = CString::new(name).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mode = (libc::S_IRUSR | libc::S_IWUSR) as libc::c_uint;
        // SAFETY: valid NUL-terminated name; the descriptor is closed below.
        let fd = unsafe { libc::shm_open(cname.as_ptr(), libc::O_RDWR | libc::O_CREAT, mode) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let result = Self::map(fd, len);
        // SAFETY: `fd` is open; the mapping stays valid after closing it.
        unsafe { libc::close(fd) };
        result
    }

    fn map(fd: libc::c_int, len: usize) -> io::Result<Self> {
        // SAFETY: `stat` is plain data filled by fstat.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: valid descriptor and out pointer.
        if unsafe { libc::fstat(fd, &mut stat) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // A fresh segment is empty; growing it zero-fills. Reading beyond a too small segment
        // would crash with SIGBUS, so it is grown too.
        let size = usize::try_from(stat.st_size).unwrap_or(0);
        if size < len {
            let want = libc::off_t::try_from(len)
                .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
            // SAFETY: valid descriptor.
            if unsafe { libc::ftruncate(fd, want) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        // SAFETY: mapping `len` bytes of a segment that is at least that large.
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            ptr: ptr.cast(),
            len,
        })
    }

    #[cfg(test)]
    pub(crate) fn unlink(name: &str) {
        if let Ok(cname) = std::ffi::CString::new(name) {
            // SAFETY: valid NUL-terminated name.
            unsafe { libc::shm_unlink(cname.as_ptr()) };
        }
    }
}

#[cfg(unix)]
impl Drop for SharedMemory {
    fn drop(&mut self) {
        // SAFETY: unmapping exactly what `map` mapped. The segment itself stays (games and
        // other clients may still use it), as in the official client.
        unsafe { libc::munmap(self.ptr.cast(), self.len) };
    }
}

#[cfg(windows)]
impl SharedMemory {
    pub(crate) fn default_name() -> String {
        "MumbleLink".to_owned()
    }

    pub(crate) fn open(name: &str, len: usize) -> io::Result<Self> {
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Memory::{
            CreateFileMappingW, FILE_MAP_ALL_ACCESS, MapViewOfFile, OpenFileMappingW,
            PAGE_READWRITE,
        };
        let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
        let size = u32::try_from(len).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: valid NUL-terminated wide name; handles are closed on every path.
        unsafe {
            let mut handle = OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, wide.as_ptr());
            if handle.is_null() {
                handle = CreateFileMappingW(
                    INVALID_HANDLE_VALUE,
                    std::ptr::null(),
                    PAGE_READWRITE,
                    0,
                    size,
                    wide.as_ptr(),
                );
            }
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let view = MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, len);
            if view.Value.is_null() {
                let err = io::Error::last_os_error();
                CloseHandle(handle);
                return Err(err);
            }
            Ok(Self {
                ptr: view.Value.cast(),
                len,
                handle,
            })
        }
    }
}

#[cfg(windows)]
impl Drop for SharedMemory {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Memory::{MEMORY_MAPPED_VIEW_ADDRESS, UnmapViewOfFile};
        // SAFETY: unmapping the view and closing the handle created in `open`.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.ptr.cast(),
            });
            CloseHandle(self.handle);
        }
    }
}
