use bytes::BytesMut;
use crossbeam_queue::ArrayQueue;
use once_cell::sync::Lazy;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{debug, trace};

// Buffer size tiers for different message patterns
const SMALL_BUFFER_SIZE: usize = 4 * 1024; // 4KB
const MEDIUM_BUFFER_SIZE: usize = 64 * 1024; // 64KB
const LARGE_BUFFER_SIZE: usize = 256 * 1024; // 256KB

// Buffer pool sizes - number of buffers per pool
const SMALL_POOL_SIZE: usize = 1024; // Increased to handle high concurrency
const MEDIUM_POOL_SIZE: usize = 128; // Increased for better hit rate
const LARGE_POOL_SIZE: usize = 32; // Doubled for reduced contention

// Size-tiered buffer pools with lock-free queues
static SMALL_BUFFER_POOL: Lazy<ArrayQueue<BytesMut>> =
    Lazy::new(|| ArrayQueue::new(SMALL_POOL_SIZE));
static MEDIUM_BUFFER_POOL: Lazy<ArrayQueue<BytesMut>> =
    Lazy::new(|| ArrayQueue::new(MEDIUM_POOL_SIZE));
static LARGE_BUFFER_POOL: Lazy<ArrayQueue<BytesMut>> =
    Lazy::new(|| ArrayQueue::new(LARGE_POOL_SIZE));

// Metrics for buffer pool usage
static SMALL_POOL_HITS: AtomicUsize = AtomicUsize::new(0);
static SMALL_POOL_MISSES: AtomicUsize = AtomicUsize::new(0);
static MEDIUM_POOL_HITS: AtomicUsize = AtomicUsize::new(0);
static MEDIUM_POOL_MISSES: AtomicUsize = AtomicUsize::new(0);
static LARGE_POOL_HITS: AtomicUsize = AtomicUsize::new(0);
static LARGE_POOL_MISSES: AtomicUsize = AtomicUsize::new(0);

/// Initialize the buffer pools with pre-allocated buffers
/// This is optional but can help reduce latency during startup
pub fn initialize_buffer_pools() {
    // Initialize small buffer pool
    for _ in 0..SMALL_POOL_SIZE {
        let buffer = BytesMut::with_capacity(SMALL_BUFFER_SIZE);
        // Ignore error (can only happen if queue is full, which is impossible here)
        let _ = SMALL_BUFFER_POOL.push(buffer);
    }

    // Initialize medium buffer pool
    for _ in 0..MEDIUM_POOL_SIZE {
        let buffer = BytesMut::with_capacity(MEDIUM_BUFFER_SIZE);
        let _ = MEDIUM_BUFFER_POOL.push(buffer);
    }

    // Initialize large buffer pool
    for _ in 0..LARGE_POOL_SIZE {
        let buffer = BytesMut::with_capacity(LARGE_BUFFER_SIZE);
        let _ = LARGE_BUFFER_POOL.push(buffer);
    }

    trace!(
        "Buffer pools initialized with {} small buffers, {} medium buffers, {} large buffers",
        SMALL_POOL_SIZE,
        MEDIUM_POOL_SIZE,
        LARGE_POOL_SIZE
    );
}

/// Get a buffer sized appropriately for the required capacity
/// This will attempt to reuse a buffer from the pool, falling back to allocation if none available
pub fn get_sized_buffer(required_size: usize) -> BytesMut {
    if required_size <= SMALL_BUFFER_SIZE {
        // Try to get buffer from small pool
        match SMALL_BUFFER_POOL.pop() {
            Some(mut buffer) => {
                SMALL_POOL_HITS.fetch_add(1, Ordering::Relaxed);
                buffer.clear();
                buffer
            }
            None => {
                SMALL_POOL_MISSES.fetch_add(1, Ordering::Relaxed);
                BytesMut::with_capacity(SMALL_BUFFER_SIZE)
            }
        }
    } else if required_size <= MEDIUM_BUFFER_SIZE {
        // Try to get buffer from medium pool
        match MEDIUM_BUFFER_POOL.pop() {
            Some(mut buffer) => {
                MEDIUM_POOL_HITS.fetch_add(1, Ordering::Relaxed);
                buffer.clear();
                buffer
            }
            None => {
                MEDIUM_POOL_MISSES.fetch_add(1, Ordering::Relaxed);
                BytesMut::with_capacity(MEDIUM_BUFFER_SIZE)
            }
        }
    } else if required_size <= LARGE_BUFFER_SIZE {
        // Try to get buffer from large pool
        match LARGE_BUFFER_POOL.pop() {
            Some(mut buffer) => {
                LARGE_POOL_HITS.fetch_add(1, Ordering::Relaxed);
                buffer.clear();
                buffer
            }
            None => {
                LARGE_POOL_MISSES.fetch_add(1, Ordering::Relaxed);
                BytesMut::with_capacity(LARGE_BUFFER_SIZE)
            }
        }
    } else {
        // For extremely large buffers, just allocate directly
        BytesMut::with_capacity(required_size)
    }
}

/// Return a buffer to the appropriate pool based on its capacity
/// This allows buffer reuse to reduce allocations
pub fn return_buffer(buffer: BytesMut) {
    let capacity = buffer.capacity();

    if capacity == SMALL_BUFFER_SIZE {
        let _ = SMALL_BUFFER_POOL.push(buffer); // Ignore if pool is full
    } else if capacity == MEDIUM_BUFFER_SIZE {
        let _ = MEDIUM_BUFFER_POOL.push(buffer);
    } else if capacity == LARGE_BUFFER_SIZE {
        let _ = LARGE_BUFFER_POOL.push(buffer);
    }
    // For other sizes, just let them get dropped
}

/// Gets a small buffer (4KB) from the pool
/// Optimized access path for common small buffer requests
pub fn get_small_buffer() -> BytesMut {
    match SMALL_BUFFER_POOL.pop() {
        Some(mut buffer) => {
            SMALL_POOL_HITS.fetch_add(1, Ordering::Relaxed);
            buffer.clear();
            buffer
        }
        None => {
            SMALL_POOL_MISSES.fetch_add(1, Ordering::Relaxed);
            BytesMut::with_capacity(SMALL_BUFFER_SIZE)
        }
    }
}

/// Gets a medium buffer (64KB) from the pool
/// Optimized access path for medium-sized responses
pub fn get_medium_buffer() -> BytesMut {
    match MEDIUM_BUFFER_POOL.pop() {
        Some(mut buffer) => {
            MEDIUM_POOL_HITS.fetch_add(1, Ordering::Relaxed);
            buffer.clear();
            buffer
        }
        None => {
            MEDIUM_POOL_MISSES.fetch_add(1, Ordering::Relaxed);
            BytesMut::with_capacity(MEDIUM_BUFFER_SIZE)
        }
    }
}

/// Gets a large buffer (256KB) from the pool
/// Optimized access path for large data transfers
pub fn get_large_buffer() -> BytesMut {
    match LARGE_BUFFER_POOL.pop() {
        Some(mut buffer) => {
            LARGE_POOL_HITS.fetch_add(1, Ordering::Relaxed);
            buffer.clear();
            buffer
        }
        None => {
            LARGE_POOL_MISSES.fetch_add(1, Ordering::Relaxed);
            BytesMut::with_capacity(LARGE_BUFFER_SIZE)
        }
    }
}

/// Log buffer pool statistics for monitoring and tuning
pub fn log_buffer_pool_stats() {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }

    let small_hits = SMALL_POOL_HITS.load(Ordering::Relaxed);
    let small_misses = SMALL_POOL_MISSES.load(Ordering::Relaxed);
    let small_total = small_hits + small_misses;
    let small_hit_rate = if small_total > 0 {
        small_hits as f64 / small_total as f64 * 100.0
    } else {
        0.0
    };

    let medium_hits = MEDIUM_POOL_HITS.load(Ordering::Relaxed);
    let medium_misses = MEDIUM_POOL_MISSES.load(Ordering::Relaxed);
    let medium_total = medium_hits + medium_misses;
    let medium_hit_rate = if medium_total > 0 {
        medium_hits as f64 / medium_total as f64 * 100.0
    } else {
        0.0
    };

    let large_hits = LARGE_POOL_HITS.load(Ordering::Relaxed);
    let large_misses = LARGE_POOL_MISSES.load(Ordering::Relaxed);
    let large_total = large_hits + large_misses;
    let large_hit_rate = if large_total > 0 {
        large_hits as f64 / large_total as f64 * 100.0
    } else {
        0.0
    };

    // Log at debug level for better visibility
    debug!(
        "Buffer pool stats - Small: {:.1}% hit rate ({}/{} hits), Medium: {:.1}% hit rate ({}/{} hits), Large: {:.1}% hit rate ({}/{} hits)",
        small_hit_rate, small_hits, small_total,
        medium_hit_rate, medium_hits, medium_total,
        large_hit_rate, large_hits, large_total
    );

    // Also log pool utilization
    let small_available = SMALL_BUFFER_POOL.len();
    let medium_available = MEDIUM_BUFFER_POOL.len();
    let large_available = LARGE_BUFFER_POOL.len();

    debug!(
        "Buffer pool utilization - Small: {}/{} available ({}%), Medium: {}/{} available ({}%), Large: {}/{} available ({}%)",
        small_available, SMALL_POOL_SIZE, small_available as f64 / SMALL_POOL_SIZE as f64 * 100.0,
        medium_available, MEDIUM_POOL_SIZE, medium_available as f64 / MEDIUM_POOL_SIZE as f64 * 100.0,
        large_available, LARGE_POOL_SIZE, large_available as f64 / LARGE_POOL_SIZE as f64 * 100.0
    );
}
