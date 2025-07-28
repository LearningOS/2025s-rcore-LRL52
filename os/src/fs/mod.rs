//! File trait & inode(dir, file, pipe, stdin, stdout)

mod inode;
mod stdio;

use core::any::Any;

use crate::mm::UserBuffer;

/// trait File for all file types
pub trait File: Send + Sync {
    /// the file readable?
    fn readable(&self) -> bool;
    /// the file writable?
    fn writable(&self) -> bool;
    /// read from the file to buf, return the number of bytes read
    fn read(&self, buf: UserBuffer) -> usize;
    /// write to the file from buf, return the number of bytes written
    fn write(&self, buf: UserBuffer) -> usize;
    /// 提供一个 as_any 方法用于向下转换，便于转换回 OSInode 类型
    fn as_any(&self) -> &dyn Any;
}

pub use inode::{list_apps, open_file, linkat, unlinkat, fstat, OSInode, OpenFlags};
pub use stdio::{Stdin, Stdout};
