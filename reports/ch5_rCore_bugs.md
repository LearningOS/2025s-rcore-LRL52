Chapter 5 框架在 debug 模式下有两个 BUG，接下来我简单讲讲 BUG 的寻找历程，

BUG 描述：切换到 ch5 的 code base，将 os 的 Makefile 改为 debug 模式，内核启动后会卡死，连 shell 都没法进入。开启 TRACE level 日志，发现经过了 `sys_fork` 和 `sys_exec` 后内核就卡死了。

一开始我以为这个 BUG 是我代码的问题，我经过了非常艰难的单步调试试错，把 卡死的位置锁定在了 `sys_exec->TaskControlBlock::exec->MemorySet::from_elf->MemorySet::push->MapArea::map->MapArea::map_one->PageTable::map->PageTable::find_pte_create->Vec::push`  这里（没办法，gdb 调试时只要赌错了选择跳过这一行就卡死得重来）。并且在执行 `push` 卡死前，我特意确认了当前 Vec 实例的 capacity 等于 size，需要扩容，因此又是 allocator 卡死了。

发现是标准库的问题，那必然问题不出现在这里。我首先把 code base 切换到了ch5 默认代码，很失望地发现官方代码竟然也是同样的问题也没法调试。

这次的卡死 BUG 其实和上次类似，都是 allocator 卡死（现象一样，但导致的原因和上次不同，卡死过程中执行的代码也和上次不同，上次是 allocator 死锁，这次请见后文分析）。于是我又开始怀疑是不是官方代码哪里没注意在 `lazy_static!` 的时候爆栈了，我开始仔细查看这次新代码里的 lazy_static 初始化，别说还真让我找到一个可疑的：

```rust
lazy_static! {
    /// Creation of initial process
    ///
    /// the name "initproc" may be changed to any other app name like "usertests",
    /// but we have user_shell, so we don't need to change it.
    pub static ref INITPROC: Arc<TaskControlBlock> = Arc::new(TaskControlBlock::new(
        get_app_data_by_name("ch5b_initproc").unwrap()
    ));
}
```

`get_app_data_by_name("ch5b_initproc")` 会返回 ELF 字节流，而 ELF size 盲猜肯定爆栈了（`kernel_boot_statck` 只有 16 KiB）。不过如果真是这个问题，上一个 chapter 应该也存在呀，于是我带着好奇调试 `get_app_data` 函数，并打印出了 `app_start` 数组，

![image-20250724013259092](https://images.lrl52.top/i/2025/07/24/image-20250724013259092-2.png)

用 GPT 算出每一行「后一项 − 前一项」的结果即是 app size，

| 行号 | 计算式                  | 差值（16 位十六进制） | 差值（十进制） |
| ---- | ----------------------- | --------------------- | -------------- |
| 1    | 0x8027f1c8 − 0x802321c0 | 0x000000000004D008    | 315 400        |
| 2    | 0x80318768 − 0x802cb9c0 | 0x000000000004CDA8    | 314 792        |
| 3    | 0x803b3cd8 − 0x803652a0 | 0x000000000004EA38    | 322 104        |
| 4    | 0x80451148 − 0x80402710 | 0x000000000004EA38    | 322 104        |
| 5    | 0x804ed478 − 0x8049f2e0 | 0x000000000004E198    | 319 896        |
| 6    | 0x80589d30 − 0x8053b610 | 0x000000000004E720    | 321 312        |
| 7    | 0x80626b68 − 0x805d8c10 | 0x000000000004DF58    | 319 320        |
| 8    | 0x806c6e28 − 0x80677060 | 0x000000000004FDC8    | 327 112        |
| 9    | 0x80765a48 − 0x80717ef8 | 0x000000000004DB50    | 318 288        |
| 10   | 0x8080ed08 − 0x807bde90 | 0x0000000000050E78    | 331 384        |

虽然 app size 确实爆栈了，但是 `get_app_data` 实现是 `core::slice::from_raw_parts` 从 `.data` 段的数据就地构造，不会发生任何拷贝，因此栈大小不会受到明显影响，我也监视了 `sp` 全程位于 `kernel_boot_stack` 范围内。

此时我想到了一个办法，我可以直接监视 `sp` 的值，通过 GDB 的 watchpoint 来暴力观察 `sp` 是否一直位于 `kernel_boot_stack` 范围内。很遗憾的是，GDB 的条件断点或 watch 很鸡肋，开了后 qemu 慢到几乎没法跑，其实哪怕慢个 10 倍都是可以接受的，但实际远不止慢 10 倍。我苦苦等了很久，也没有等来 watchpoint 触发，并且此时的 QEMU 跑了半天该打印的信息都还没输出。

那我只能在代码里插桩判断了，考虑到可能是由于 stackoverflow 导致 allocator 死锁，于是我在里面添加了 `check_allocator` 检查函数，并把它放在各个关键关键位置：

```rust
/// check if the heap allocator is locked
pub fn check_allocator() {
    if HEAP_ALLOCATOR.is_locked() {
        log::error!("Heap allocator has been locked!");
    } else {
        log::info!("Heap allocator is unlocked now.");
    }
}
```

加了这段代码后，我发现在经过了 70+ 次 `check_allocator()` 触发后，最后卡死在了`log::info!` 这句上面。于是我有经历了艰难痛苦的单步调试，调试 log 和 std 源代码里的函数（这确实是个不太明智的决定，不过让我也体验到了调试标准库的经验，调试时 GDB 可能无法正常找到源代码文件，但栈帧上会显示源代码文件名和行号，此时就需要再 vscode 里手动翻找源代码对照着看）。

不过意外之中，我进入了卡死循环的代码。QEMU 会不断死循环执行 `0xFFFFFFFFFFFFF000` 和 `0xFFFFFFFFFFFFF004` 这两个地址处的代码。这就奇怪了，因为 `0xFFFFFFFFFFFFF000` 恰好是 `TRAMPOLINE` 代码，而这段代码应该是从用户态陷入内核态时才会执行的，内核里发生 trap 不应该跳到 `trap_from_kernel` 吗？

![image-20250724140639713](https://images.lrl52.top/i/2025/07/24/image-20250724140639713-2.png)

观察 scause 为 0xf 表示发生 trap 的原因是 Store/AMO page fault。sp 此时等于为 `0xffffffffffff9f98`，可以推断出是 pid = 1 的 app 爆内核栈了，其内核栈范围应该为 `[0xffffffffffffa000, 0xffffffffffffc000)`，并且现在跳到了用户态才该进入的 trap_handler（`__alltraps`），

![image-20250724140756678](https://images.lrl52.top/i/2025/07/24/image-20250724140756678-2.png)

打印出寄存器，可以发现 `ra` 又与 allocator 有关，

![image-20250724035336484](https://images.lrl52.top/i/2025/07/24/image-20250724035336484-2.png)

现在除了内核爆栈的问题，现在 rCore 又有新 BUG 了‼️，为什么在 kernel 里没有跳到内核 trap 的入口，为什么 $stvec 没有被设置为 `trap_from_kernel(0x8021f5e6)`。

接下来调试 `set_kernel_trap_entry` 函数，发现该函数调用后 $stval 没有被修改。接下来进一步指令级单步调试，问题出现在 `csrw stvec, a0` 这条指令上，

![image-20250724142657201](https://images.lrl52.top/i/2025/07/24/image-20250724142657201-2.png)

GDB `ni` 执行后，我竟惊奇地发现 $stvec 没有被更改？

![image-20250724143201736](https://images.lrl52.top/i/2025/07/24/image-20250724143201736-2.png)

通过询问 GPT 和查询 RSIC-V 手册可知，写入的值 `a0` 没有 4 字节对齐，写入操作被 QEMU/RUST-SBI 直接忽略。 也即是 `set_kernel_trap_entry` 函数地址在编译时没有保证 4 字节对齐。

![image-20250724144359890](https://images.lrl52.top/i/2025/07/24/image-20250724144359890-2.png)

Rust 不支持在函数前加个什么 align 标注就能让地址自动对齐。为了在内核态下 $stvec 能写入地址并正常跳转，参照 `__alltraps` ，新增一段汇编代码，给 `trap_from_kernel` 添加一个 wrapper 函数 `_trap_from_kernel`，新增 `os/src/trap/kerneltrap.S`，

```assembly
.globl _trap_from_kernel
.globl trap_from_kernel
.align 2
_trap_from_kernel:
    j trap_from_kernel
```

然后修改 `os/src/trap/mod.rs`，

```rust
global_asm!(include_str!("trap.S"));
global_asm!(include_str!("kerneltrap.S"));

fn set_kernel_trap_entry() {
    extern "C" {
        /// This function is defined in `kerneltrap.S` and
        /// the address is aligned to 4 bytes
        fn _trap_from_kernel();
    }
    unsafe {
        stvec::write(_trap_from_kernel as usize, TrapMode::Direct);
    }
}
```

经过这样折腾后，`set_kernel_trap_entry` 函数终于写入成功，终于能够正常跳转了。但内核启动后依然卡死，而不是预计的跳转到 `trap_from_kernel` 然后 panic 终止。通过调试发现，虽然确实跳到 `trap_from_kernel` 了，但接下来会修改 `sp` 分配栈帧，而此时 `sp` 已经爆栈了，访存又会触发 Store/AMO page fault，于是继续陷入死循环。

此时我已经大概猜到爆栈原因了，于是这次我先让 GDB 停留在最后两次 `check_allocator` 输出的位置，然后给 $sp 设置 watchpoint 。呜呜呜，这次终于找到 BUG 现场了，也即发生 stackoverflow 的指令位置，

![image-20250724162715489](https://images.lrl52.top/i/2025/07/24/image-20250724162715489-2.png)

此时我们可以发现其 stack 深度非常大，

![image-20250724163047135](https://images.lrl52.top/i/2025/07/24/image-20250724163047135-2.png)

至此，困扰了我一天的 rCore 框架的两个 BUG 终于解决了。解决办法是增大 `os/src/config.rs` 的 `KERNEL_STACK_SIZE` ，

```rust
/// kernel stack size
pub const KERNEL_STACK_SIZE: usize = 4096 * 16;
```

注，`boot_stack` 的大小可以在 `os/src/entry.asm` 修改 `.space 4096 * 16`：

```assembly
    .section .text.entry
    .globl _start
_start:
    la sp, boot_stack_top
    call rust_main

    .section .bss.stack
    .globl boot_stack_lower_bound
boot_stack_lower_bound:
    .space 4096 * 16
    .globl boot_stack_top
boot_stack_top:
```