//! File and filesystem-related syscalls
use easy_fs::Stat;
use crate::fs::{fstat, linkat, open_file, unlinkat, OSInode, OpenFlags};
use crate::mm::{translated_byte_buffer, translated_str, UserBuffer};
use crate::task::{current_task, current_user_token};

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_write", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        if !file.writable() {
            return -1;
        }
        let file = file.clone();
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        file.write(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_read(fd: usize, buf: *const u8, len: usize) -> isize {
    trace!("kernel:pid[{}] sys_read", current_task().unwrap().pid.0);
    let token = current_user_token();
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if let Some(file) = &inner.fd_table[fd] {
        let file = file.clone();
        if !file.readable() {
            return -1;
        }
        // release current task TCB manually to avoid multi-borrow
        drop(inner);
        trace!("kernel: sys_read .. file.read");
        file.read(UserBuffer::new(translated_byte_buffer(token, buf, len))) as isize
    } else {
        -1
    }
}

pub fn sys_open(path: *const u8, flags: u32) -> isize {
    trace!("kernel:pid[{}] sys_open", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let token = current_user_token();
    let path = translated_str(token, path);
    if let Some(inode) = open_file(path.as_str(), OpenFlags::from_bits(flags).unwrap()) {
        let mut inner = task.inner_exclusive_access();
        let fd = inner.alloc_fd();
        inner.fd_table[fd] = Some(inode);
        fd as isize
    } else {
        -1
    }
}

pub fn sys_close(fd: usize) -> isize {
    trace!("kernel:pid[{}] sys_close", current_task().unwrap().pid.0);
    let task = current_task().unwrap();
    let mut inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() {
        return -1;
    }
    if inner.fd_table[fd].is_none() {
        return -1;
    }
    inner.fd_table[fd].take();
    0
}

/// YOUR JOB: Implement fstat.
pub fn sys_fstat(fd: usize, st: *mut Stat) -> isize {
    trace!("kernel:pid[{}] sys_fstat",current_task().unwrap().pid.0);
    let token = current_user_token();
    let buffer = translated_byte_buffer(token, st as *const u8, core::mem::size_of::<Stat>());
    let task = current_task().unwrap();
    let inner = task.inner_exclusive_access();
    if fd >= inner.fd_table.len() || inner.fd_table[fd].is_none() {
        trace!("kernel: sys_fstat failed, fd out of range or not opened");
        return -1;
    }
    let file_arc = inner.fd_table[fd].as_ref().unwrap();
    let any_arc = file_arc.as_any();
    let inode = any_arc.downcast_ref::<OSInode>();
    if inode.is_none() {
        trace!("kernel: sys_fstat failed, fd is not an OSInode");
        return -1;
    }
    let result = fstat(inode.unwrap());
    
    // Copy the Stat to user space
    let src = unsafe {
        core::slice::from_raw_parts(&result as *const Stat as *const u8, core::mem::size_of::<Stat>())
    };
    let mut offset = 0;
    for dst in buffer {
        let len = dst.len();
        dst.copy_from_slice(&src[offset..offset + len]);
        offset += len;
    }
    0
}

/// YOUR JOB: Implement linkat.
pub fn sys_linkat(old_name: *const u8, new_name: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_linkat", current_task().unwrap().pid.0);
    let token = current_user_token();
    let old_name = translated_str(token, old_name);
    let new_name = translated_str(token, new_name);
    if old_name == new_name {
        trace!("kernel: sys_linkat failed, old and new names are the same: {}", old_name);
        return -1;
    }
    let ip = open_file(&old_name, OpenFlags::RDONLY);
    if ip.is_none() {
        trace!("kernel: sys_linkat failed, old file not found: {}", old_name);
        return -1;
    }
    linkat(ip.unwrap(), new_name)
}

/// YOUR JOB: Implement unlinkat.
pub fn sys_unlinkat(name: *const u8) -> isize {
    trace!("kernel:pid[{}] sys_unlinkat", current_task().unwrap().pid.0);
    let token = current_user_token();
    let name = translated_str(token, name);
    let inode = open_file(&name, OpenFlags::RDONLY);
    if inode.is_none() {
        trace!("kernel: sys_unlinkat failed, file not found: {}", name);
        return -1;
    }
    drop(inode);
    unlinkat(name)
}
