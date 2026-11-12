#![doc(html_no_source)]

//! 可组合的手推单调时钟（composable manually refreshed monotonic clocks）。
//!
//! 当前通过 [`global`] 使用进程唯一时间轴（process-global timeline）。
//! 先显式初始化（explicit initialization），再读取或真实刷新；本库不依赖异步运行时。

#[cfg(not(target_has_atomic = "64"))]
compile_error!("手推核心（manual core）需要64位原子能力（64-bit atomic capability）");

#[cfg(target_has_atomic = "64")]
pub mod global;

#[cfg(all(feature = "auto-refresh", any(target_os = "windows", target_os = "linux", target_os = "android")))]
pub mod auto_refresh;
