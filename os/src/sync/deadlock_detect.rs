use alloc::vec::Vec;
use crate::task::current_task;

/// A deadlock detector for mutexes and semaphores
pub struct DeadlockDetector {
    /// Whether the deadlock detector is enabled
    pub enabled: bool,
    /// Detector for mutex deadlocks
    pub mutex_detector: Option<DeadlockDetectStruct>,
    /// Detector for semaphore deadlocks
    pub semaphore_detector: Option<DeadlockDetectStruct>,
}

impl DeadlockDetector {
    /// Create a new deadlock detector
    pub fn new() -> Self {
        Self {
            enabled: false,
            mutex_detector: Some(DeadlockDetectStruct::new()),
            semaphore_detector: Some(DeadlockDetectStruct::new()),
        }
    }

    /// add a thread to the deadlock detector
    pub fn add_thread(&mut self, tid: usize) {
        if let Some(detector) = self.mutex_detector.as_mut() {
            detector.add_thread(tid);
        }
        if let Some(detector) = self.semaphore_detector.as_mut() {
            detector.add_thread(tid);
        }
    }
    
    /// detect deadlock for mutexes and semaphores
    pub fn detect_deadlock(&self) -> bool {
        if let Some(detector) = self.mutex_detector.as_ref() {
            if detector.detect_deadlock() {
                return true;
            }
        }
        if let Some(detector) = self.semaphore_detector.as_ref() {
            if detector.detect_deadlock() {
                return true;
            }
        }
        false
    }
}

pub struct DeadlockDetectStruct {
    available: Vec<usize>,
    allocation: Vec<Vec<usize>>,
    need: Vec<Vec<usize>>,
}

impl DeadlockDetectStruct {
    pub fn new() -> Self {
        let mut v: Vec<Vec<usize>> = Vec::new();
        v.resize(1, Vec::new()); // Initialize with one thread
        Self {
            available: Vec::new(),
            allocation: v.clone(),
            need: v.clone(),
        }
    }

    pub fn create_resource(&mut self, id: usize, res_count: usize) {
        let m = self.available.len();
        if id >= m {
            self.available.resize(id + 1, 0);
            for v in self.allocation.iter_mut() {
                v.resize(id + 1, 0);
            }
            for v in self.need.iter_mut() {
                v.resize(id + 1, 0);
            }
        }
        self.available[id] = res_count;
    }

    fn add_thread(&mut self, tid: usize) {
        let m = self.available.len();  // current number of resources
        let n = self.allocation.len(); // current number of threads
        if tid >= n {
            let mut v: Vec<usize> = Vec::<usize>::new();
            v.resize(m, 0);
            self.allocation.resize(tid + 1, v.clone());
            self.need.resize(tid + 1, v.clone());
        }
        self.allocation[tid].fill(0);
        self.need[tid].fill(0);
    }

    pub fn request(&mut self, id: usize) {
        let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
        self.need[tid][id] += 1;
    }

    pub fn revoke_request(&mut self, id: usize) {
        let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
        assert!(self.need[tid][id] > 0, "No request to revoke");
        self.need[tid][id] -= 1;
    }

    pub fn alloc(&mut self, id: usize) {
        let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
        self.revoke_request(id);
        self.allocation[tid][id] += 1;
        self.available[id] -= 1;
    }

    pub fn dealloc(&mut self, id: usize) {
        let tid = current_task().unwrap().inner_exclusive_access().res.as_ref().unwrap().tid;
        assert!(self.allocation[tid][id] > 0, "No allocation to deallocate");
        self.allocation[tid][id] -= 1;
        self.available[id] += 1;
    }

    fn detect_deadlock(&self) -> bool {
        let m = self.available.len();  // number of resources
        let n = self.allocation.len(); // number of threads
        let mut work = self.available.clone();
        let mut finish = Vec::new();
        finish.resize(n, false);

        loop {
            let mut found = false;
            for i in 0..n {
                // If thread i is not finished and its needs can be satisfied with current available resources
                if finish[i] == false && self.need[i].iter().zip(&work).all(|(need, work)| need <= work) {
                    for j in 0..m {
                        work[j] += self.allocation[i][j]; // Add allocation of thread i to work
                    }
                    finish[i] = true; // Mark thread i as finished
                    found = true;
                }
            }
            if !found {
                break; // No more threads can be finished
            }
        }

        // If any thread is not finished, we have a deadlock
        return finish.iter().any(|&f| !f);
    }
}