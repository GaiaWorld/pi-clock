//! 非线程安全错误（non-thread-safe error）可局部派生（derive），不得进入跨线程报告（cross-thread report）。
use pi_result::prelude::*;
#[derive(Debug, ThisError)]
#[error("local {0:?}")]
struct Local(std::rc::Rc<u8>);
fn main() {
    let _ = Report::new(Local(std::rc::Rc::new(1)));
}
