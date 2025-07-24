//!Implementation of [`TaskManager`]
use core::cmp::Ordering;
use super::TaskControlBlock;
use crate::sync::UPSafeCell;
use alloc::collections::binary_heap::BinaryHeap;
// use alloc::collections::VecDeque;
use alloc::sync::Arc;
use lazy_static::*;

struct ReadyQueueItem {
    task: Arc<TaskControlBlock>,
    stride: Stride,
}

impl PartialEq for ReadyQueueItem {
    fn eq(&self, other: &Self) -> bool {
        self.stride == other.stride
    }
}

impl PartialOrd for ReadyQueueItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.stride.partial_cmp(&other.stride)
    }
}

impl Ord for ReadyQueueItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.stride.cmp(&other.stride)
    }
}

impl Eq for ReadyQueueItem {}

///A array of `TaskControlBlock` that is thread-safe
pub struct TaskManager {
    // ready_queue: VecDeque<Arc<TaskControlBlock>>,
    ready_queue: BinaryHeap<ReadyQueueItem>,
}


#[derive(Copy, Clone)]
/// Stride for stride scheduling algorithm
pub struct Stride(pub u64);

#[derive(Copy, Clone)]
/// Priority for stride scheduling algorithm
pub struct Priority(pub u64);

impl Stride {
    pub const BIG_STRIDE: u64 = u64::MAX;
    pub const HALF: u64 = Stride::BIG_STRIDE / 2;

    pub fn init() -> Self {
        Stride(0)
    }
}

impl PartialEq for Stride {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

impl PartialOrd for Stride {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Stride {
    fn cmp(&self, other: &Self) -> Ordering {
        // 计算 (self - other) 在模 2⁶⁴ 环上的差值
        let diff = self.0.wrapping_sub(other.0);

        // -------------- 关键逻辑 ----------------
        // 若 diff ≤ HALF ：说明 self 实际比 other 大（落后），
        // 若 diff > HALF ：说明发生回绕，self 实际比 other 小（领先）。
        //
        // BinaryHeap 是大根堆；为了让 pop() 返回“最小 stride 的进程”，
        // 反转顺序，让“较小的”stride 更大。（BinaryHeap 大根堆）
        if diff <= Stride::HALF {
            Ordering::Less      // self 落后 → 视为“较小”，排在后面
        } else {
            Ordering::Greater   // self 领先 → 视为“较大”，优先弹出
        }
    }
}

impl Eq for Stride {}

impl Priority {
    /// Default priority for stride scheduling algorithm
    pub fn init() -> Self {
        Priority(16)
    }
}

/// A simple FIFO scheduler.   (x)
/// A simple stride scheduler. (√)
impl TaskManager {
    ///Creat an empty TaskManager
    pub fn new() -> Self {
        Self {
            // ready_queue: VecDeque::new(),
            ready_queue: BinaryHeap::new(),
        }
    }
    /// Add process back to ready queue
    pub fn add(&mut self, task: Arc<TaskControlBlock>) {
        // self.ready_queue.push_back(task);
        let stride = task.inner_exclusive_access().stride;
        self.ready_queue.push(ReadyQueueItem {
            task,
            stride,
        });
    }
    /// Take a process out of the ready queue
    pub fn fetch(&mut self) -> Option<Arc<TaskControlBlock>> {
        // self.ready_queue.pop_front()
        let item = self.ready_queue.pop();
        if item.is_none() {
            return None;
        }
        let ret = item.unwrap().task;
        {
            let mut inner = ret.inner_exclusive_access();
            inner.stride.0 = inner.stride.0.wrapping_add(Stride::BIG_STRIDE / inner.priority.0);
        }
        Some(ret)
    }
}

lazy_static! {
    /// TASK_MANAGER instance through lazy_static!
    pub static ref TASK_MANAGER: UPSafeCell<TaskManager> =
        unsafe { UPSafeCell::new(TaskManager::new()) };
}

/// Add process to ready queue
pub fn add_task(task: Arc<TaskControlBlock>) {
    //trace!("kernel: TaskManager::add_task");
    TASK_MANAGER.exclusive_access().add(task);
}

/// Take a process out of the ready queue
pub fn fetch_task() -> Option<Arc<TaskControlBlock>> {
    //trace!("kernel: TaskManager::fetch_task");
    TASK_MANAGER.exclusive_access().fetch()
}
