//! 路径覆盖（path override）必须是Rust路径（Rust path），字符串（string）必须拒绝编译（compile fail）。
use pi_result::prelude::*;
#[derive(Debug, ThisError)]
#[pi_result(crate = "pi_result")]
#[error("invalid path")]
struct Invalid;
fn main() {}
