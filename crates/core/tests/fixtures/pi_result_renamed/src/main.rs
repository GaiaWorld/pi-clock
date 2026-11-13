//! 真实重命名依赖（actual renamed dependency）由宏（macro）发现，不用代码别名（code alias）冒充。
use errors::prelude::*;
#[derive(Debug, ThisError)]
#[error("renamed {0}")]
struct Renamed(u32);
fn main() {
    let report: Error = Report::new(Renamed(19)).change_context(ErrorKind::Dependency);
    assert_eq!(report.downcast_ref::<Renamed>().unwrap().0, 19);
    assert_eq!(report.current_kind(), &ErrorKind::Dependency);
    println!("PI_CLOCK_RENAMED_OK");
}
