//! 依赖的夜间能力（dependency nightly capability）不代开目标语言特性（target language feature）。
use pi_result::prelude::*;
#[derive(Debug, ThisError)]
#[error("provided")]
struct Provided { backtrace: std::backtrace::Backtrace }
fn main() { let _ = Provided { backtrace: std::backtrace::Backtrace::disabled() }; }
