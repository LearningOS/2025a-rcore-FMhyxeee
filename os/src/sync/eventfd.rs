//! Eventfd implementation
//!
//! Eventfd is a Linux system call that creates a file descriptor for event notification.
//! It can be used for thread synchronization and signal mechanisms.

use alloc::collections::VecDeque;
use alloc::sync::Arc;

use crate::task::{block_current_and_run_next, current_task, TaskControlBlock};
use crate::sync::UPSafeCell;

/// Eventfd flags
pub const EFD_SEMAPHORE: i32 = 1;
pub const EFD_NONBLOCK: i32 = 2048;

/// EventFd structure
pub struct EventFd {
    /// The inner state of the EventFd
    inner: UPSafeCell<EventFdInner>,
}

/// Inner state of EventFd
pub struct EventFdInner {
    /// Current counter value
    count: u64,
    /// Flags
    flags: i32,
    /// Wait queue for readers
    reader_queue: VecDeque<Arc<TaskControlBlock>>,
    /// Wait queue for writers
    writer_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl EventFd {
    /// Create a new EventFd with initial value and flags
    pub fn new(initval: u32, flags: i32) -> Self {
        Self {
            inner: unsafe { UPSafeCell::new(EventFdInner {
                count: initval as u64,
                flags,
                reader_queue: VecDeque::new(),
                writer_queue: VecDeque::new(),
            }) },
        }
    }

    /// Read from EventFd
    /// Returns the value read, or -2 if non-blocking and no data available
    pub fn read(&self) -> isize {
        let mut inner = self.inner.exclusive_access();

        // If count is 0 and non-blocking mode, return error
        if inner.count == 0 && (inner.flags & EFD_NONBLOCK != 0) {
            return -2; // EAGAIN
        }

        // Wait for count to be non-zero
        while inner.count == 0 {
            let current_task = current_task().unwrap();
            inner.reader_queue.push_back(current_task);
            drop(inner);
            block_current_and_run_next();
            inner = self.inner.exclusive_access();
        }

        // In semaphore mode, always read 1, otherwise read the full count
        let value = if inner.flags & EFD_SEMAPHORE != 0 {
            inner.count -= 1;
            1
        } else {
            let value = inner.count;
            inner.count = 0;
            value
        };

        // Wake up any waiting writers
        if !inner.writer_queue.is_empty() {
            let task = inner.writer_queue.pop_front().unwrap();
            drop(inner);
            add_task(task);
        } else {
            drop(inner);
        }

        value as isize
    }

    /// Write to EventFd
    /// Returns 0 on success, or -2 if non-blocking and would overflow
    pub fn write(&self, value: u64) -> isize {
        let mut inner = self.inner.exclusive_access();

        // Check for overflow (max value - count)
        let max_value = u64::MAX - 1;
        if value > max_value - inner.count {
            // Would overflow
            if inner.flags & EFD_NONBLOCK != 0 {
                return -2; // EAGAIN
            }
            // In blocking mode, we could wait, but for simplicity return error
            return -1;
        }

        // Add the value to counter
        inner.count += value;

        // Wake up any waiting readers
        if !inner.reader_queue.is_empty() {
            let task = inner.reader_queue.pop_front().unwrap();
            drop(inner);
            add_task(task);
        } else {
            drop(inner);
        }

        0
    }

    /// Get current count (for debugging)
    pub fn get_count(&self) -> u64 {
        self.inner.exclusive_access().count
    }
}

/// Add a task to the ready queue
fn add_task(task: Arc<TaskControlBlock>) {
    use crate::task::add_task;
    add_task(task);
}