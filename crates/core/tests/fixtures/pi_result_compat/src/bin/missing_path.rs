//! 不可达路径（unreachable path）不得静默回退（silent fallback）。
use pi_result::prelude::*;
#[derive(Debug, ThisError)]
#[pi_result(crate = ::missing_runtime)]
#[error("missing runtime")]
struct Missing;
fn main() {}
