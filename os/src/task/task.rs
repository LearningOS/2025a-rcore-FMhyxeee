//! Types related to task management

use super::TaskContext;

/// Maximum number of syscalls to track (covers syscall IDs 0-499)
const MAX_SYSCALL_NUM: usize = 500;

/// The task control block (TCB) of a task.
#[derive(Copy, Clone)]
pub struct TaskControlBlock {
    /// The task status in it's lifecycle
    pub task_status: TaskStatus,
    /// The task context
    pub task_cx: TaskContext,
    /// System call count statistics for this task
    pub syscall_count: [usize; MAX_SYSCALL_NUM]
}

impl TaskControlBlock {
    /// Create a new task control block.
    pub fn new() -> Self {
        Self {
            task_status: TaskStatus::UnInit,
            task_cx: TaskContext::zero_init(),
            syscall_count: [0; MAX_SYSCALL_NUM],
        }
    }

    /// Increment the count for a specific syscall
    pub fn inc_syscall_count(&mut self, syscall_id: usize) {
        if syscall_id < MAX_SYSCALL_NUM {
            self.syscall_count[syscall_id] += 1;
        }
    }
    
    /// Get the count for a specific syscall
    pub fn get_syscall_count(&self, syscall_id: usize) -> usize {
        if syscall_id < MAX_SYSCALL_NUM {
            self.syscall_count[syscall_id]
        } else {
            0
        }
    }
}

/// The status of a task
#[derive(Copy, Clone, PartialEq)]
pub enum TaskStatus {
    /// uninitialized
    UnInit,
    /// ready to run
    Ready,
    /// running
    Running,
    /// exited
    Exited,
}
