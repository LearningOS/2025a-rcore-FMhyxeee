//! Process management syscalls
use crate::{
    task::{exit_current_and_run_next, suspend_current_and_run_next, get_current_syscall_count},
    timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("[kernel] Application exited with code {}", exit_code);
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// get time with second and microsecond
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    let us = get_time_us();
    unsafe {
        *ts = TimeVal {
            sec: us / 1_000_000,
            usec: us % 1_000_000,
        };
    }
    0
}

/// sys_trace system call implementation
/// 
/// This syscall has three different functions based on trace_request:
/// - trace_request = 0: Read a byte from memory address `id`
/// - trace_request = 1: Write `data` (as u8) to memory address `id`
/// - trace_request = 2: Get syscall count for syscall ID `id`
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace request={}, id={}, data={}", trace_request, id, data);
    
    match trace_request {
        // Read a byte from memory address
        0 => {
            unsafe {
                // SAFETY: As per assignment requirements, no safety checks needed
                let ptr = id as *const u8;
                *ptr as isize
            }
        }
        
        // Write a byte to memory address
        1 => {
            unsafe {
                // SAFETY: As per assignment requirements, no safety checks needed
                let ptr = id as *mut u8;
                *ptr = data as u8;
                0
            }
        }
        
        // Get syscall count for the specified syscall ID
        2 => {
            // Note: This call itself should be counted, which is handled in the syscall dispatcher
            get_current_syscall_count(id) as isize
        }
        
        // Invalid trace_request
        _ => -1,
    }
}