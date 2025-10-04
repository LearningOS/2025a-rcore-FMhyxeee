//! Types related to task management & Functions for completely changing TCB
use super::TaskContext;
use super::{kstack_alloc, pid_alloc, KernelStack, PidHandle};
use crate::config::TRAP_CONTEXT_BASE;
use crate::mm::{MemorySet, PhysPageNum, VirtAddr, KERNEL_SPACE, VPNRange};
use crate::sync::UPSafeCell;
use crate::trap::{trap_handler, TrapContext};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::cell::RefMut;

/// Task control block structure
///
/// Directly save the contents that will not change during running
pub struct TaskControlBlock {
    // Immutable
    /// Process identifier
    pub pid: PidHandle,

    /// Kernel stack corresponding to PID
    pub kernel_stack: KernelStack,

    /// Mutable
    inner: UPSafeCell<TaskControlBlockInner>,
}

impl TaskControlBlock {
    /// Get the mutable reference of the inner TCB
    pub fn inner_exclusive_access(&self) -> RefMut<'_, TaskControlBlockInner> {
        self.inner.exclusive_access()
    }
    /// Get the address of app's page table
    pub fn get_user_token(&self) -> usize {
        let inner = self.inner_exclusive_access();
        inner.memory_set.token()
    }
}

pub struct TaskControlBlockInner {
    /// The physical page number of the frame where the trap context is placed
    pub trap_cx_ppn: PhysPageNum,

    /// Application data can only appear in areas
    /// where the application address space is lower than base_size
    pub base_size: usize,

    /// Save task context
    pub task_cx: TaskContext,

    /// Maintain the execution status of the current process
    pub task_status: TaskStatus,

    /// Application address space
    pub memory_set: MemorySet,

    /// Parent process of the current process.
    /// Weak will not affect the reference count of the parent
    pub parent: Option<Weak<TaskControlBlock>>,

    /// A vector containing TCBs of all child processes of the current process
    pub children: Vec<Arc<TaskControlBlock>>,

    /// It is set when active exit or execution error occurs
    pub exit_code: i32,

    /// Heap bottom
    pub heap_bottom: usize,

    /// Program break
    pub program_brk: usize,

    /// Process priority for stride scheduling (default: 16)
    pub priority: usize,

    /// Current stride value for stride scheduling
    pub stride: usize,
}

impl TaskControlBlockInner {
    /// get the trap context
    pub fn get_trap_cx(&self) -> &'static mut TrapContext {
        self.trap_cx_ppn.get_mut()
    }
    /// get the user token
    pub fn get_user_token(&self) -> usize {
        self.memory_set.token()
    }
    fn get_status(&self) -> TaskStatus {
        self.task_status
    }
    pub fn is_zombie(&self) -> bool {
        self.get_status() == TaskStatus::Zombie
    }
}

impl TaskControlBlock {
    /// Create a new process
    ///
    /// At present, it is only used for the creation of initproc
    pub fn new(elf_data: &[u8]) -> Self {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = kstack_alloc();
        let kernel_stack_top = kernel_stack.get_top();
        // push a task context which goes to trap_return to the top of kernel stack
        let task_control_block = Self {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: user_sp,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: None,
                    children: Vec::new(),
                    exit_code: 0,
                    heap_bottom: user_sp,
                    program_brk: user_sp,
                    priority: 16,
                    stride: 0,
                })
            },
        };
        // prepare TrapContext in user space
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            kernel_stack_top,
            trap_handler as usize,
        );
        task_control_block
    }

    /// Load a new elf to replace the original application address space and start execution
    pub fn exec(&self, elf_data: &[u8]) {
        // memory_set with elf program headers/trampoline/trap context/user stack
        let (memory_set, user_sp, entry_point) = MemorySet::from_elf(elf_data);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();

        // **** access current TCB exclusively
        let mut inner = self.inner_exclusive_access();
        // substitute memory_set
        inner.memory_set = memory_set;
        // update trap_cx ppn
        inner.trap_cx_ppn = trap_cx_ppn;
        // initialize base_size
        inner.base_size = user_sp;
        // initialize trap_cx
        let trap_cx = inner.get_trap_cx();
        *trap_cx = TrapContext::app_init_context(
            entry_point,
            user_sp,
            KERNEL_SPACE.exclusive_access().token(),
            self.kernel_stack.get_top(),
            trap_handler as usize,
        );
        // **** release inner automatically
    }

    /// parent process fork the child process
    pub fn fork(self: &Arc<Self>) -> Arc<Self> {
        // ---- access parent PCB exclusively
        let mut parent_inner = self.inner_exclusive_access();
        // copy user space(include trap context)
        let memory_set = MemorySet::from_existed_user(&parent_inner.memory_set);
        let trap_cx_ppn = memory_set
            .translate(VirtAddr::from(TRAP_CONTEXT_BASE).into())
            .unwrap()
            .ppn();
        // alloc a pid and a kernel stack in kernel space
        let pid_handle = pid_alloc();
        let kernel_stack = kstack_alloc();
        let kernel_stack_top = kernel_stack.get_top();
        let task_control_block = Arc::new(TaskControlBlock {
            pid: pid_handle,
            kernel_stack,
            inner: unsafe {
                UPSafeCell::new(TaskControlBlockInner {
                    trap_cx_ppn,
                    base_size: parent_inner.base_size,
                    task_cx: TaskContext::goto_trap_return(kernel_stack_top),
                    task_status: TaskStatus::Ready,
                    memory_set,
                    parent: Some(Arc::downgrade(self)),
                    children: Vec::new(),
                    exit_code: 0,
                    heap_bottom: parent_inner.heap_bottom,
                    program_brk: parent_inner.program_brk,
                    priority: parent_inner.priority,
                    stride: 0,
                })
            },
        });
        // add child
        parent_inner.children.push(task_control_block.clone());
        // modify kernel_sp in trap_cx
        // **** access child PCB exclusively
        let trap_cx = task_control_block.inner_exclusive_access().get_trap_cx();
        trap_cx.kernel_sp = kernel_stack_top;
        // return
        task_control_block
        // **** release child PCB
        // ---- release parent PCB
    }

    /// get pid of process
    pub fn getpid(&self) -> usize {
        self.pid.0
    }

    /// change the location of the program break. return None if failed.
    pub fn change_program_brk(&self, size: i32) -> Option<usize> {
        let mut inner = self.inner_exclusive_access();
        let heap_bottom = inner.heap_bottom;
        let old_break = inner.program_brk;
        let new_brk = inner.program_brk as isize + size as isize;
        if new_brk < heap_bottom as isize {
            return None;
        }
        let result = if size < 0 {
            inner
                .memory_set
                .shrink_to(VirtAddr(heap_bottom), VirtAddr(new_brk as usize))
        } else {
            inner
                .memory_set
                .append_to(VirtAddr(heap_bottom), VirtAddr(new_brk as usize))
        };
        if result {
            inner.program_brk = new_brk as usize;
            Some(old_break)
        } else {
            None
        }
    }

    /// 实现 mmap 系统调用：将物理内存映射到指定的虚拟地址范围
    /// 参数：
    /// - start: 虚拟地址起始位置（必须页对齐）
    /// - len: 映射长度（字节）
    /// - prot: 内存保护标志（第0位=可读，第1位=可写，第2位=可执行）
    /// 返回：成功返回 0，失败返回 -1
    pub fn mmap(&self, start: usize, len: usize, prot: usize) -> isize {
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
        if self.inner_exclusive_access().memory_set.check_range_conflict(start_va, end_va) {
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
        self.inner_exclusive_access().memory_set.insert_framed_area(start_va, end_va, map_perm);
        
        0  // 成功
    }

    /// 实现 munmap 系统调用：取消指定虚拟地址范围的内存映射
    /// 参数：
    /// - start: 虚拟地址起始位置（必须页对齐）
    /// - len: 取消映射的长度（字节）
    /// 返回：成功返回 0，失败返回 -1
    pub fn munmap(&self, start: usize, len: usize) -> isize {
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
            match self.inner_exclusive_access().memory_set.translate(vpn) {
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
        if !self.inner_exclusive_access().memory_set.remove_area_range(start_va, end_va) {
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
    Zombie,
}
