//! 没有直接依赖（direct dependency）时，缺少路径覆盖（path override）必须明确失败。
use bridge::pi_result::prelude::*;
#[derive(Debug, ThisError)]
#[error("missing override")]
struct Missing;
fn main() {}
