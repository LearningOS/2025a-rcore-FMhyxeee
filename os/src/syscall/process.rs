//! Process management syscalls
use crate::mm::translated_byte_buffer;
use crate::task::{change_program_brk, current_user_token, exit_current_and_run_next, get_current_syscall_count, suspend_current_and_run_next};
use crate::timer::get_time_us;

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(_exit_code: i32) -> ! {
    trace!("kernel: sys_exit");
    exit_current_and_run_next();
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel: sys_yield");
    suspend_current_and_run_next();
    0
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    
    // 获取当前时间（微秒）
    let us = get_time_us();
    let sec = us / 1_000_000;  // 转换为秒
    let usec = us % 1_000_000; // 剩余的微秒
    
    // 创建 TimeVal 结构体
    let time_val = TimeVal { sec, usec };
    
    // 使用虚拟内存管理将时间数据写入用户空间
    // 获取用户页表令牌
    let token = current_user_token();
    
    // 将 TimeVal 结构体转换为字节数组
    let time_val_bytes = unsafe {
        core::slice::from_raw_parts(
            &time_val as *const TimeVal as *const u8,
            core::mem::size_of::<TimeVal>(),
        )
    };
    
    // 获取用户空间的缓冲区（处理可能的跨页情况）
    let buffers = translated_byte_buffer(
        token,
        ts as *const u8,
        core::mem::size_of::<TimeVal>(),
    );
    
    // 将时间数据复制到用户空间缓冲区
    let mut offset = 0;
    for buffer in buffers {
        let copy_len = buffer.len().min(time_val_bytes.len() - offset);
        if copy_len > 0 {
            buffer[..copy_len].copy_from_slice(&time_val_bytes[offset..offset + copy_len]);
            offset += copy_len;
        }
        if offset >= time_val_bytes.len() {
            break;
        }
    }
    
    0 // 成功返回 0
}

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    match trace_request {
        // Read a byte from memory address
        0 => {
            unsafe {
                
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

// YOUR JOB: Implement mmap.
pub fn sys_mmap(_start: usize, _len: usize, _port: usize) -> isize {
    trace!("kernel: sys_mmap NOT IMPLEMENTED YET!");
    -1
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(_start: usize, _len: usize) -> isize {
    trace!("kernel: sys_munmap NOT IMPLEMENTED YET!");
    -1
}
/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel: sys_sbrk");
    if let Some(old_brk) = change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}
