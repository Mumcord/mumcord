//! Pre-allocated frame buffers.
//!
//! Capture threads run in the real-time path (OS camera callbacks), so they must not allocate or
//! lock. All buffers are allocated when the pool is created; taking and returning one only moves a
//! pointer through a lock-free queue.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use crossbeam_queue::ArrayQueue;

/// A fixed set of equally sized byte buffers shared between one capture session and its consumers.
#[derive(Clone)]
pub struct FramePool {
    shared: Arc<Shared>,
}

struct Shared {
    free: ArrayQueue<Box<[u8]>>,
    buffer_len: usize,
}

impl FramePool {
    /// Allocates `buffers` buffers of `buffer_len` bytes each, up front.
    ///
    /// # Panics
    ///
    /// When `buffers` is 0: a pool without buffers could never deliver a frame.
    pub fn new(buffers: usize, buffer_len: usize) -> Self {
        assert!(buffers > 0, "a frame pool needs at least one buffer");
        let free = ArrayQueue::new(buffers);
        for _ in 0..buffers {
            // The queue was sized for exactly this many buffers, so the push cannot fail.
            let _ = free.push(vec![0u8; buffer_len].into_boxed_slice());
        }
        Self {
            shared: Arc::new(Shared { free, buffer_len }),
        }
    }

    /// Takes a free buffer, or `None` when all buffers are still in use. Callers drop the frame
    /// in that case: the consumer is behind, and waiting would stall the camera.
    ///
    /// The buffer keeps the bytes of its previous frame; writers overwrite all of it.
    pub fn acquire(&self) -> Option<PooledBuffer> {
        self.shared.free.pop().map(|data| PooledBuffer {
            data,
            home: Arc::clone(&self.shared),
        })
    }

    /// Size of every buffer in bytes.
    pub fn buffer_len(&self) -> usize {
        self.shared.buffer_len
    }

    /// Buffers that are free right now.
    pub fn available(&self) -> usize {
        self.shared.free.len()
    }

    /// Total number of buffers.
    pub fn capacity(&self) -> usize {
        self.shared.free.capacity()
    }
}

/// A buffer borrowed from a [`FramePool`]; dropping it hands it back.
pub struct PooledBuffer {
    data: Box<[u8]>,
    home: Arc<Shared>,
}

impl Deref for PooledBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.data
    }
}

impl DerefMut for PooledBuffer {
    fn deref_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

impl Drop for PooledBuffer {
    fn drop(&mut self) {
        // An empty boxed slice does not allocate. The push cannot fail: every buffer in the queue
        // or out of it belongs to this pool, so there is always room for it to come back.
        let data = std::mem::take(&mut self.data);
        let _ = self.home.free.push(data);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hands_out_each_buffer_once_and_takes_it_back_on_drop() {
        let pool = FramePool::new(2, 16);
        let a = pool.acquire().expect("first buffer");
        let b = pool.acquire().expect("second buffer");
        assert_eq!((a.len(), b.len()), (16, 16));
        assert!(pool.acquire().is_none(), "the pool is empty");
        assert_eq!(pool.available(), 0);

        drop(a);
        assert_eq!(pool.available(), 1);
        let c = pool.acquire().expect("returned buffer");
        assert_eq!(c.len(), 16);
        drop((b, c));
        assert_eq!(pool.available(), pool.capacity());
    }

    #[test]
    fn buffers_come_back_from_other_threads() {
        let pool = FramePool::new(3, 8);
        let taken: Vec<_> = (0..3).map(|_| pool.acquire().expect("buffer")).collect();
        std::thread::spawn(move || drop(taken))
            .join()
            .expect("consumer thread");
        assert_eq!(pool.available(), 3);
    }

    #[test]
    fn buffers_outlive_the_pool_handle() {
        let pool = FramePool::new(1, 4);
        let mut buffer = pool.acquire().expect("buffer");
        drop(pool);
        buffer[0] = 7;
        assert_eq!(buffer[0], 7);
    }
}
