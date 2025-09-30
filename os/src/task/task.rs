//! Types related to task management
use alloc::collections::btree_map::BTreeMap;

use super::TaskContext;
use crate::config::TRAP_CONTEXT_BASE;
use crate::mm::{
    kernel_stack_position, MapPermission, MemorySet, PhysPageNum, VPNRange, VirtAddr, KERNEL_SPACE
};
use crate::trap::{trap_handler, TrapContext};

/// The task control block (TCB) of a task.
pub struct TaskControlBlock {
    /// Save task context
    pub task_cx: TaskContext,

    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,

    /// Application address space
    pub memory_set: MemorySet,

    /// The phys page number of trap context
    pub trap_cx_ppn: PhysPageNum,

    /// The size(top addr) of program which is loaded from elf file
    pub base_size: usize,

    /// Heap bottom
    pub heap_bottom: usize,

    /// Program break
    pub program_brk: usize,

    /// System call count statistics for this task
    /// because we 
    pub syscall_count: BTreeMap<usize, usize>,
}

impl TaskControlBlock {
    /// get the trap context
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    /// get the user token
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    /// Based on the elf info in program, build the contents of task in a new address space
    pub fn new(elf_data: &[u8], app_id: usize) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        let task_status = TaskStatus::Ready;
        // map a kernel-stack in kernel space
        let (kernel_stack_bottom, kernel_stack_top) = kernel_stack_position(app_id);
        KERNEL_SPACE.exclusive_access().insert_framed_area(
            kernel_stack_bottom.into(),
            kernel_stack_top.into(),
            MapPermission::R | MapPermission::W,
        );
        let task_control_block = Self {
            task_status,
            task_cx: TaskContext::goto_trap_return(kernel_stack_top),
            memory_set,
            trap_cx_ppn,
            base_size: user_sp,
            heap_bottom: user_sp,
            program_brk: user_sp,
            syscall_count: BTreeMap::new(),
        };
        // prepare TrapContext in user space
        let trap_cx = task_control_block.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }
    /// change the location of the program break. return None if failed.
    pub fn change_program_brk(&mut self, size: i32) -> Option<usize> {
        let old_break = self.program_brk;
        let new_brk = self.program_brk as isize + size as isize;
        if new_brk < self.heap_bottom as isize {
            return None;
        }
        let result = if size < 0 {
            self.memory_set
                .shrink_to(VirtAddr(self.heap_bottom), VirtAddr(new_brk as usize))
        } else {
            self.memory_set
                .append_to(VirtAddr(self.heap_bottom), VirtAddr(new_brk as usize))
        };
        if result {
            self.program_brk = new_brk as usize;
            Some(old_break)
        } else {
            None
        }
    }

    /// Increment the syscall count for a given syscall ID
    pub fn inc_syscall_count(&mut self, syscall_id: usize) {
        let count = self.syscall_count.entry(syscall_id).or_insert(0);
        *count += 1;
    }

    /// Get the syscall count for a given syscall ID
    pub fn get_syscall_count(&self, syscall_id: usize) -> usize {
        *self.syscall_count.get(&syscall_id).unwrap_or(&0)  
    }

    /// 实现 mmap 系统调用：将物理内存映射到指定的虚拟地址范围
    /// 参数：
    /// - start: 虚拟地址起始位置（必须页对齐）
    /// - len: 映射长度（字节）
    /// - prot: 内存保护标志（第0位=可读，第1位=可写，第2位=可执行）
    /// 返回：成功返回 0，失败返回 -1
    pub fn mmap(&mut self, start: usize, len: usize, prot: usize) -> isize {
        use crate::config::PAGE_SIZE;
        use crate::mm::{VirtAddr, MapPermission};
        
        // 参数验证
        // 1. 检查 start 是否页对齐
        if start % PAGE_SIZE != 0 {
            return -1;
        }
        
        // 2. 检查 prot 的有效性
        if prot & !0x7 != 0 {  // prot 其余位必须为0
            return -1;
        }
        
        if prot & 0x7 == 0 {   // 这样的内存无意义
            return -1;
        }
        
        // 3. 如果 len 为 0，直接返回成功
        if len == 0 {
            return 0;
        }
        
        // 计算结束地址（向上取整到页边界）
        let end = start + len;
        let end_aligned = (end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        
        let start_va = VirtAddr::from(start);
        let end_va = VirtAddr::from(end_aligned);
        
        // 4. 检查地址范围是否与现有映射冲突
        if self.memory_set.check_range_conflict(start_va, end_va) {
            return -1;
        }
        
        // 构建内存权限
        let mut map_perm = MapPermission::U; // 用户模式可访问
        if prot & 0x1 != 0 {  // 可读
            map_perm |= MapPermission::R;
        }
        if prot & 0x2 != 0 {  // 可写
            map_perm |= MapPermission::W;
        }
        if prot & 0x4 != 0 {  // 可执行
            map_perm |= MapPermission::X;
        }
        
        // 执行内存映射
        self.memory_set.insert_framed_area(start_va, end_va, map_perm);
        
        0  // 成功
    }

    /// 实现 munmap 系统调用：取消指定虚拟地址范围的内存映射
    /// 参数：
    /// - start: 虚拟地址起始位置（必须页对齐）
    /// - len: 取消映射的长度（字节）
    /// 返回：成功返回 0，失败返回 -1
    pub fn munmap(&mut self, start: usize, len: usize) -> isize {
        use crate::config::PAGE_SIZE;
        use crate::mm::VirtAddr;
        
        // 参数验证
        // 1. 检查 start 是否页对齐
        if start % PAGE_SIZE != 0 {
            return -1;
        }
        
        // 2. 如果 len 为 0，直接返回成功
        if len == 0 {
            return 0;
        }
        
        // 计算结束地址（向上取整到页边界）
        let end = start + len;
        let end_aligned = (end + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
        
        let start_va = VirtAddr::from(start);
        let end_va = VirtAddr::from(end_aligned);
        
        // 3. 检查要取消映射的页面范围是否都已映射
        // 如果范围内包含未映射的页面，则返回错误
        let start_vpn = start_va.floor();
        let end_vpn = end_va.ceil();
        
        for vpn in VPNRange::new(start_vpn, end_vpn) {
            match self.memory_set.translate(vpn) {
                Some(pte) => {
                    if !pte.is_valid() {
                        return -1;
                    }
                }
                None => {
                    return -1;
                }
            }
        }
        
        // 4. 执行实际的取消映射
        if !self.memory_set.remove_area_range(start_va, end_va) {
            return -1;  // 范围内存在未映射的虚拟内存
        }
        
        0  // 成功
    }
}

#[derive(Copy, Clone, PartialEq)]
/// task status: UnInit, Ready, Running, Exited
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
