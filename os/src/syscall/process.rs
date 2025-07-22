//! Process management syscalls
use crate::{mm::{translated_byte_buffer, MapPermission, PageTable, PhysAddr, VPNRange, VirtAddr, VirtPageNum, FRAME_ALLOCATOR}, task::{change_program_brk, current_tcb, current_user_token, exit_current_and_run_next, get_syscall_count, suspend_current_and_run_next}, timer::get_time_us};

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

/// TODO: Finish sys_trace to pass testcases
/// HINT: You might reimplement it with virtual memory management.
pub fn sys_trace(trace_request: usize, id: usize, data: usize) -> isize {
    trace!("kernel: sys_trace");
    match trace_request {
        // TraceRequest::READ 
        0 => {
            let token = current_user_token();
            let page_table = PageTable::from_token(token);
            if !VirtAddr::is_valid(id) {
                return -1;
            }
            let va = VirtAddr::from(id);
            let vpn = va.floor();
            let result = page_table.translate(vpn);
            if result.is_none() {
                return -1;
            }
            let pte = result.unwrap();
            if !pte.is_valid() || !pte.readable() {
                return -1;
            }
            let pa: PhysAddr = (Into::<usize>::into(Into::<PhysAddr>::into(pte.ppn())) 
                                + va.page_offset()).into();
            *pa.get_mut::<u8>() as isize
        },
        // TraceRequest::WRITE
        1 => {
            let token = current_user_token();
            let page_table = PageTable::from_token(token);
            if !VirtAddr::is_valid(id) {
                return -1;
            }
            let va = VirtAddr::from(id);
            let vpn = va.floor();
            let result = page_table.translate(vpn);
            if result.is_none() {
                return -1;
            }
            let pte = result.unwrap();
            if !pte.is_valid() || !pte.writable() {
                return -1;
            }
            let pa: PhysAddr = (Into::<usize>::into(Into::<PhysAddr>::into(pte.ppn())) 
                                + va.page_offset()).into();
            *pa.get_mut::<u8>() = data as u8;
            0
        },
        2 => {
            get_syscall_count(id)
        },
        _ => {
            trace!("Unsupported trace request: {}", trace_request);
            -1 // Unsupported request
        }
    }
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
    let current = current_tcb();
    let new_vpn_range = VPNRange::new(start_vpn, end_vpn);
    for area in current.memory_set.areas.iter() {
        if area.vpn_range.has_intersection(&new_vpn_range) {
            trace!("kernel: sys_mmap failed, overlapping memory area!");
            return -1;
        }
    }

    let mut permission = MapPermission::U;
    permission |= MapPermission::from_bits((prot << 1) as u8).unwrap();
    current.memory_set.insert_framed_area(start_va, end_va, permission);
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
    
    let current = current_tcb();
    match current.memory_set.remove_framed_area(start_va, end_va) {
        Some(_) => 0,
        None => -1
    }
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
