#![doc(html_no_source)]

//! 时钟（clock）组合所需的基础契约（foundational contracts）。
//!
//! 通过 [`source::MonotonicSource`] 接入具体单调来源（monotonic source），
//! 由消费者（consumer）保存样本（sample）并计算经过时长（elapsed duration）。

pub mod source;
