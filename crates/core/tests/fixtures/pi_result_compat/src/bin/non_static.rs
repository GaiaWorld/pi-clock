//! 非静态借用（non-static borrow）不能通过派生（derive）逃逸到报告（report）。
use pi_result::prelude::*;
#[derive(Debug, ThisError)]
#[error("borrowed {0}")]
struct Borrowed<'a>(&'a str);
fn main() {
    let text = String::from("temporary");
    let report = Report::new(Borrowed(&text));
    drop(report);
}
