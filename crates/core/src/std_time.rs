//! 标准库时间类型的兼容入口（standard-library time compatibility entry）。
//!
//! 这里的三个名称与 `std::time` 中的原类型（original types）相同，可直接
//! 传入和返回要求标准库类型的接口（APIs），无需包装、换算或分配。
//! `Duration` 表示非负时长（nonnegative duration），`Instant` 表示标准库的
//! 单调时间点（monotonic instant），`SystemTime` 表示系统日期时间点
//! （system-time instant）。它们不是本库手推时钟（manually refreshed clock）
//! 的时间点类型（time-point types）。
//!
//! 标准库固有方法（inherent methods）、运算符（operators）、错误返回和恐慌
//! （panic）条件保持原有语义；本模块（module）不新增可失败操作。
//! 调用 `Instant::now()` 或 `SystemTime::now()` 仍使用标准库来源，成本与可用性
//! 取决于目标平台（target platform）和宿主（host），不会读取本库手推缓存。
//! 单独使用这些类型不会初始化本库全局时钟（global clock）或启动维护线程
//! （maintenance thread）。
//!
//! 本库时间点与 `Instant` 或 `SystemTime` 的转换（conversion）须由对应能力
//! 提供足够的起点映射（origin mapping）、同域及范围／精度检查；不能从一个裸
//! 纳秒数（nanosecond count）推定等价。后续本库新增的可失败边界使用 pi_result。
//!
//! 以下示例（example）只演示类型等价（type identity），不读取系统时间：
//!
//! ```
//! use pi_clock_core::std_time::{Duration, Instant, SystemTime};
//!
//! let elapsed = Duration::from_nanos(1);
//! let standard_elapsed: std::time::Duration = elapsed;
//! assert_eq!(standard_elapsed, std::time::Duration::from_nanos(1));
//!
//! fn standard_instant(value: Instant) -> std::time::Instant {
//!     value
//! }
//! fn standard_system_time(value: SystemTime) -> std::time::SystemTime {
//!     value
//! }
//! ```

// 内部设计（internal design）：直接重导出（re-export）保留标准类型身份及全部既有
// 实现（implementations），不引入新类型（newtype）或双向转换算法。
// 这里不维护起点、样本身份、缓存或全局对象；标准库行为与本库受检映射分开负责。
// 不改source模块对std::time::Duration的现有引用，两条路径解析为同一个Rust类型。

// @trace 1
#[doc(no_inline)]
pub use std::time::Duration;

// @trace 2
#[doc(no_inline)]
pub use std::time::Instant;

// @trace 3
#[doc(no_inline)]
pub use std::time::SystemTime;
