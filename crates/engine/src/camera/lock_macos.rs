//! Screen-lock events on macOS (FR-8.21): the distributed notification
//! `com.apple.screenIsLocked`, observed on the main thread, whose run loop the app runs.

use std::ptr::NonNull;

use block2::RcBlock;
use dispatch2::DispatchQueue;
use objc2_foundation::{NSDistributedNotificationCenter, NSNotification, NSString};

/// Registers the observer on the main thread; it calls `on_lock` for each lock and stays for
/// the life of the process.
pub(super) fn watch(on_lock: fn()) {
    DispatchQueue::main().exec_async(move || {
        let block = RcBlock::new(move |_: NonNull<NSNotification>| on_lock());
        let name = NSString::from_str("com.apple.screenIsLocked");
        let center = NSDistributedNotificationCenter::defaultCenter();
        // SAFETY: no sender filter, delivery on the posting (main) thread; the block only calls
        // a plain function.
        let observer = unsafe {
            center.addObserverForName_object_queue_usingBlock(Some(&name), None, None, &block)
        };
        // The listener runs as long as the process: the observer is never removed.
        std::mem::forget(observer);
    });
}
