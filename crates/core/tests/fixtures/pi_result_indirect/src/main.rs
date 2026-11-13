//! 间接重导出（indirect re-export）在派生后（after derive）明确选择真实可达路径（reachable path）。
use bridge::pi_result::prelude::*;
#[derive(Debug, ThisError)]
#[pi_result(crate = ::bridge::pi_result)]
#[error("indirect {0}")]
struct Indirect(u32);
fn main() {
    let report: Error = Report::new(Indirect(29)).change_context(ErrorKind::Dependency);
    assert_eq!(report.downcast_ref::<Indirect>().unwrap().0, 29);
    assert_eq!(report.current_kind(), &ErrorKind::Dependency);
    println!("PI_CLOCK_INDIRECT_OK");
}
