#![doc(html_no_source)]

//! 真实平台来源（real platform sources），供上层时钟（clocks）组合使用。
//!
//! [`monotonic`] 负责固定单调计量基准（monotonic measurement basis）；
//! 时钟的相对起点（relative origin）、缓存（cache）及推进（refresh）由消费者拥有。
//! 浏览器来源（browser source）通过 `browser` 功能特性（feature）显式启用。
//!
//! 普通平台单调来源（ordinary platform monotonic source）的函数体（function bodies）已实现；
//! 当前已有Windows x86_64的构建（build）、功能测试（functional tests）和独立复核（independent review）证据。
//! 最低支持版本（MSRV）、其他目标（targets）及最低系统（minimum OS）的运行验收（runtime acceptance）分别确认。

pub mod monotonic;
