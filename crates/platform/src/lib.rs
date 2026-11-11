#![doc(html_no_source)]

//! 真实平台来源（real platform sources），供上层时钟（clocks）组合使用。
//!
//! [`monotonic`] 负责固定单调计量基准（monotonic measurement basis）；
//! 时钟的相对起点（relative origin）、缓存（cache）及推进（refresh）由消费者拥有。
//! 浏览器来源（browser source）通过 `browser` 功能特性（feature）显式启用。
//!
//! 当前只有声明与连接，取时实现（sampling implementation）仍为未实现占位
//! （unimplemented stubs），尚不能用作取时服务（time service）。

pub mod monotonic;
