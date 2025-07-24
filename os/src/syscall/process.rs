//! Process management syscalls
use alloc::sync::Arc;

use crate::{
    loader::get_app_data_by_name,
    mm::{translated_byte_buffer, translated_refmut, translated_str, MapPermission, VPNRange, VirtAddr, VirtPageNum, FRAME_ALLOCATOR},
    task::{
        add_task, current_task, current_user_token, exit_current_and_run_next, suspend_current_and_run_next, Priority, TaskControlBlock
    }, timer::get_time_us,
};

#[repr(C)]
#[derive(Debug)]
pub struct TimeVal {
    pub sec: usize,
    pub usec: usize,
}

/// task exits and submit an exit code
pub fn sys_exit(exit_code: i32) -> ! {
    trace!("kernel:pid[{}] sys_exit", current_task().unwrap().pid.0);
    exit_current_and_run_next(exit_code);
    panic!("Unreachable in sys_exit!");
}

/// current task gives up resources for other tasks
pub fn sys_yield() -> isize {
    trace!("kernel:pid[{}] sys_yield", current_task().unwrap().pid.0);
    suspend_current_and_run_next();
    0
}

pub fn sys_getpid() -> isize {
    trace!("kernel: sys_getpid pid:{}", current_task().unwrap().pid.0);
    current_task().unwrap().pid.0 as isize
}

pub fn sys_fork() -> isize {
    trace!("kernel:pid[{}] sys_fork", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let new_task = current_task.fork();
    let new_pid = new_task.pid.0;
    // modify trap context of new_task, because it returns immediately after switching
    let trap_cx = new_task.inner_exclusive_access().get_trap_cx();
    // we do not have to move to next instruction since we have done it before
    // for child process, fork returns 0
    trap_cx.x[10] = 0;
    // add new task to scheduler
    add_task(new_task);
    new_pid as isize
}

pub fn sys_exec(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_exec", current_task().unwrap().pid.0);
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(data) = get_app_data_by_name(path.as_str()) {
        let task = current_task().unwrap();
        task.exec(data);
        0
    } else {
        -1
    }
}

/// If there is not a child process whose pid is same as given, return -1.
/// Else if there is a child process but it is still running, return -2.
pub fn sys_waitpid(pid: isize, exit_code_ptr: *mut i32) -> isize {
    trace!("kernel::pid[{}] sys_waitpid [{}]", current_task().unwrap().pid.0, pid);
    let task = current_task().unwrap();
    // find a child process

    // ---- access current PCB exclusively
    let mut inner = task.inner_exclusive_access();
    if !inner
        .children
        .iter()
        .any(|p| pid == -1 || pid as usize == p.getpid())
    {
        return -1;
        // ---- release current PCB
    }
    let pair = inner.children.iter().enumerate().find(|(_, p)| {
        // ++++ temporarily access child PCB exclusively
        p.inner_exclusive_access().is_zombie() && (pid == -1 || pid as usize == p.getpid())
        // ++++ release child PCB
    });
    if let Some((idx, _)) = pair {
        let child = inner.children.remove(idx);
        // confirm that child will be deallocated after being removed from children list
        assert_eq!(Arc::strong_count(&child), 1);
        let found_pid = child.getpid();
        // ++++ temporarily access child PCB exclusively
        let exit_code = child.inner_exclusive_access().exit_code;
        // ++++ release child PCB
        *translated_refmut(inner.memory_set.token(), exit_code_ptr) = exit_code;
        found_pid as isize
    } else {
        -2
    }
    // ---- release current PCB automatically
}

/// YOUR JOB: get time with second and microsecond
/// HINT: You might reimplement it with virtual memory management.
/// HINT: What if [`TimeVal`] is splitted by two pages ?
pub fn sys_get_time(ts: *mut TimeVal, _tz: usize) -> isize {
    trace!("kernel: sys_get_time");
    // let token = current_user_token();
    // let page_table = PageTable::from_token(token);
    // let va = VirtAddr::from(ts as usize);
    // let vpn = va.floor();
    // let ppn = page_table.translate(vpn).unwrap().ppn();
    // let offset = va.page_offset();
    // let pa: PhysAddr = (Into::<usize>::into(Into::<PhysAddr>::into(ppn)) + offset).into();
    // let ts: &mut TimeVal = pa.get_mut();
    // let us = get_time_us();
    // *ts = TimeVal {
    //     sec: us / 1_000_000,
    //     usec: us % 1_000_000,
    // };
    // 上面这个实现并不完美，无法处理跨页的情况
    let token = current_user_token();
    let buf = translated_byte_buffer(token, ts as *const u8, core::mem::size_of::<TimeVal>());
    let us = get_time_us();
    let ts = TimeVal {
        sec: us / 1_000_000,
        usec: us % 1_000_000,
    };
    let src = unsafe {
        core::slice::from_raw_parts(&ts as *const TimeVal as *const u8, core::mem::size_of::<TimeVal>())
    };
    let mut offset = 0;
    for dst in buf {
        let len = dst.len();
        dst.copy_from_slice(&src[offset..offset + len]);
        offset += len;
    }
    0
}

// YOUR JOB: Implement mmap.
pub fn sys_mmap(start: usize, len: usize, prot: usize) -> isize {
    trace!("kernel: sys_mmap");
    let start_va = VirtAddr::from(start);
    if !start_va.aligned() || (prot & !0x7 != 0) || (prot & 0x7 == 0) {
        trace!("kernel: sys_mmap failed, invalid parameters!");
        return -1;
    }
    let end_va: VirtAddr = VirtAddr::from(start + len).ceil().into();
    let start_vpn: VirtPageNum = start_va.into();
    let end_vpn: VirtPageNum = end_va.into();

    // Check if there are enough free pages
    if end_vpn.0 - start_vpn.0 > FRAME_ALLOCATOR.exclusive_access().free_page_count() {
        trace!("kernel: sys_mmap failed, not enough free pages!");
        return -1;
    }

    // Check if the new memory area overlaps with existing areas
    let current = current_task().unwrap();
    let mut inner = current.inner_exclusive_access();
    let new_vpn_range = VPNRange::new(start_vpn, end_vpn);
    for area in inner.memory_set.areas.iter() {
        if area.vpn_range.has_intersection(&new_vpn_range) {
            trace!("kernel: sys_mmap failed, overlapping memory area!");
            return -1;
        }
    }

    let mut permission = MapPermission::U;
    permission |= MapPermission::from_bits((prot << 1) as u8).unwrap();
    inner.memory_set.insert_framed_area(start_va, end_va, permission);
    0
}

// YOUR JOB: Implement munmap.
pub fn sys_munmap(start: usize, len: usize) -> isize {
    trace!("kernel: sys_munmap");
    let start_va = VirtAddr::from(start);
    if !start_va.aligned() {
        trace!("kernel: sys_munmap failed, invalid parameters!");
        return -1;
    }
    let end_va: VirtAddr = VirtAddr::from(start + len).ceil().into();
    
    let current = current_task().unwrap();
    let mut inner = current.inner_exclusive_access();
    match inner.memory_set.remove_framed_area(start_va, end_va) {
        Some(_) => 0,
        None => -1
    }
}

/// change data segment size
pub fn sys_sbrk(size: i32) -> isize {
    trace!("kernel:pid[{}] sys_sbrk", current_task().unwrap().pid.0);
    if let Some(old_brk) = current_task().unwrap().change_program_brk(size) {
        old_brk as isize
    } else {
        -1
    }
}

/// YOUR JOB: Implement spawn.
/// HINT: fork + exec =/= spawn
pub fn sys_spawn(path: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_spawn", current_task().unwrap().pid.0);
    let current_task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    let elf_data = get_app_data_by_name(&path);
    if elf_data.is_none() {
        trace!("kernel: sys_spawn failed, app not found: {}", path);
        return -1;
    }
    let new_task = Arc::new(TaskControlBlock::new(elf_data.unwrap()));

    // set the parent of the new task to the current task
    let mut parent_inner = current_task.inner_exclusive_access();
    parent_inner.children.push(new_task.clone());
    let mut child_inner = new_task.inner_exclusive_access();
    child_inner.parent = Some(Arc::downgrade(&current_task));
    drop(child_inner); // 提前释放 child_inner，因为 add_task 也需要获取可变引用

    // add the new task to the scheduler
    add_task(new_task.clone());
    
    new_task.pid.0 as isize
}

// YOUR JOB: Set task priority.
pub fn sys_set_priority(prio: isize) -> isize {
    trace!("kernel:pid[{}] sys_set_priority", current_task().unwrap().pid.0);
    if prio < 2 {
        trace!("kernel: sys_set_priority failed, invalid priority: {}", prio);
        return -1;
    }
    let current_task = current_task().unwrap();
    let mut inner = current_task.inner_exclusive_access();
    inner.priority = Priority(prio as u64);
    prio
}
