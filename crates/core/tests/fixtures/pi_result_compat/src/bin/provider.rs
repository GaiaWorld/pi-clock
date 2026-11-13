//! 夜间提供者（nightly provider）由目标自身启用语言特性（target language feature）。
#![feature(error_generic_member_access)]
use pi_result::prelude::*;
#[derive(Debug, ThisError)]
#[error("provided")]
struct Provided { backtrace: std::backtrace::Backtrace }
fn main() {
    let error = Provided { backtrace: std::backtrace::Backtrace::disabled() };
    let trace = std::error::request_ref::<std::backtrace::Backtrace>(&error).unwrap();
    assert!(std::ptr::eq(trace, &error.backtrace));
    println!("PI_CLOCK_PROVIDER_OK");
}
