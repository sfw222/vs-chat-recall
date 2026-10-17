//! A global allocator wrapper that tracks live/peak/total heap bytes.
//!
//! Used by `bench` to report peak heap usage without OS-specific APIs.
//! The numbers are "bytes handed out by this process's allocator", which is a
//! stable, reproducible metric — unlike RSS, which includes loader and
//! unrelated runtime memory.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
pub static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);
pub static TOTAL_BYTES: AtomicUsize = AtomicUsize::new(0);

pub struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            TOTAL_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
            PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
        if !new_ptr.is_null() {
            if new_size >= layout.size() {
                let grow = new_size - layout.size();
                let live = LIVE_BYTES.fetch_add(grow, Ordering::Relaxed) + grow;
                TOTAL_BYTES.fetch_add(grow, Ordering::Relaxed);
                PEAK_BYTES.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE_BYTES.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

/// Reset all counters. Call once before a measurement run.
pub fn reset() {
    LIVE_BYTES.store(0, Ordering::Relaxed);
    PEAK_BYTES.store(0, Ordering::Relaxed);
    TOTAL_BYTES.store(0, Ordering::Relaxed);
}

/// Re-baseline peak to the current live value (for per-file peak measurement).
pub fn reset_peak() {
    PEAK_BYTES.store(LIVE_BYTES.load(Ordering::Relaxed), Ordering::Relaxed);
}

pub fn live() -> usize {
    LIVE_BYTES.load(Ordering::Relaxed)
}

pub fn peak() -> usize {
    PEAK_BYTES.load(Ordering::Relaxed)
}

pub fn total() -> usize {
    TOTAL_BYTES.load(Ordering::Relaxed)
}
