//! 独立下游（independent downstream）仅直接依赖pi_result，验证真实兼容派生（compatible derive）。
use pi_result::prelude::*;
use std::error::Error as StdError;

#[derive(Debug, ThisError)]
#[error("read failed: {source}")]
struct ReadError {
    #[from]
    source: std::io::Error,
}

#[derive(Debug, ThisError)]
#[error(transparent)]
struct Transparent(ReadError);

#[derive(Debug, ThisError)]
#[error("outer {cause}")]
struct WithSource {
    #[source]
    cause: ReadError,
}

#[derive(Debug, ThisError)]
#[error("load context")]
struct LoadContext;

#[derive(Debug, ThisError)]
#[error("{label}: {value}")]
struct Borrowed<'a, T: std::fmt::Display + std::fmt::Debug> {
    label: &'a str,
    value: T,
}

#[derive(Debug, ThisError)]
#[error("local {0:?}")]
struct Local(std::rc::Rc<std::cell::Cell<u8>>);

#[derive(Debug, pi_result::thiserror::Error)]
#[error("legacy")]
struct Legacy;

#[derive(Debug, pi_result::ThisError)]
#[error("root")]
struct Root;

/// 裸来源错误（raw source error）经标准转换（From）及问号操作（question-mark operator）到达边界（boundary）。
fn read(fail: bool) -> RawResult<u32, ReadError> {
    if fail {
        Err::<(), _>(std::io::Error::new(std::io::ErrorKind::NotFound, "missing sample"))?;
    }
    Ok(41)
}

/// 上下文和附件（context and attachments）沿真实报告链（report chain）传播。
fn load(fail: bool) -> Result<u32> {
    let value = read(fail).attach(Operation("load_clock_config"))
        .into_typed(LoadContext).into_error(ErrorKind::Dependency)?;
    Ok(value + 1)
}

fn main() {
    assert_eq!(load(false).unwrap(), 42);
    let report: Error = load(true).unwrap_err();
    assert_eq!(report.current_kind(), &ErrorKind::Dependency);
    assert!(report.contains_context::<LoadContext>());
    let original = report.downcast_ref::<ReadError>().unwrap();
    assert_eq!(original.to_string(), "read failed: missing sample");
    let io = original.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
    assert!(std::ptr::eq(io, &original.source));
    assert_eq!(io.kind(), std::io::ErrorKind::NotFound);
    assert_eq!(report.attachment_ref::<Operation>(), Some(&Operation("load_clock_config")));

    let wrapped = Transparent(std::io::Error::new(std::io::ErrorKind::NotFound, "transparent").into());
    assert_eq!(wrapped.to_string(), "read failed: transparent");
    assert!(std::ptr::eq(wrapped.source().unwrap().downcast_ref::<std::io::Error>().unwrap(), &wrapped.0.source));
    let outer = WithSource { cause: std::io::Error::new(std::io::ErrorKind::Other, "explicit").into() };
    assert!(std::ptr::eq(outer.source().unwrap().downcast_ref::<ReadError>().unwrap(), &outer.cause));

    let label = String::from("borrowed");
    let borrowed = Borrowed { label: &label, value: 7 };
    assert_eq!(borrowed.to_string(), "borrowed: 7");
    let _: &dyn StdError = &borrowed;
    let local = Local(std::rc::Rc::new(std::cell::Cell::new(3)));
    local.0.set(4);
    let _: &dyn StdError = &local;
    assert_eq!(local.0.get(), 4);
    assert_eq!(Legacy.to_string(), "legacy");
    assert_eq!(Root.to_string(), "root");
    let _: ErrorReport<ErrorKind> = Report::new(Root).change_context(ErrorKind::Other);
    println!("PI_CLOCK_DERIVE_COMPAT_OK");
}
