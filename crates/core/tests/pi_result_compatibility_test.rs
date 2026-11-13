//! 通过公开来源契约（public source contract）检查兼容错误派生（compatible error derive）。
//! 来源、解释上下文和附件（source/context/attachment）必须经过真实报告链（report chain）保留；
//! 正常分支（success branch）也经过同一调用者（caller），不改变产品来源实现（source implementation）。

use pi_clock_core::source::{MonotonicSource, SourceCapabilities, SuspendBehavior};
use pi_result::prelude::*;
use std::error::Error as StdError;
use std::time::Duration;

#[derive(Debug, ThisError)]
#[error("sample failed: {source}")]
struct SampleError {
    #[from]
    source: std::io::Error,
}

#[derive(Debug, ThisError)]
#[error("measure source")]
struct MeasureContext;

#[derive(Debug, PartialEq, Eq)]
struct SampleEvidence(u32);

/// 确定的外部来源（deterministic external source）；仅实现已有公开契约（public contract），
/// 不接触全局时钟（global clock）或安装任何测试钩子（test hook）。
struct Source {
    fail: bool,
}

impl MonotonicSource for Source {
    type Sample = u64;

    fn try_sample(&self) -> Result<Self::Sample> {
        if self.fail {
            let raw: RawResult<u64, SampleError> = Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "sample denied",
            ).into());
            raw.attach_opaque(SampleEvidence(17)).into_error(ErrorKind::Dependency)
        } else {
            Ok(23)
        }
    }

    fn try_elapsed(&self, earlier: &Self::Sample, later: &Self::Sample) -> Result<Duration> {
        later.checked_sub(*earlier)
            .map(Duration::from_nanos)
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput))
    }

    fn capabilities(&self) -> SourceCapabilities {
        SourceCapabilities { suspend: SuspendBehavior::Unspecified }
    }
}

/// 真实泛型消费者（generic consumer）使用既有公开签名（public signature）及问号传播（question-mark propagation）。
fn measure<S: MonotonicSource>(source: &S) -> Result<Duration> {
    let earlier = source.try_sample()
        .attach(Operation("sample_start"))
        .into_typed(MeasureContext)
        .into_error(ErrorKind::Unavailable)?;
    let later = source.try_sample()?;
    source.try_elapsed(&earlier, &later)
}

/// 正常来源（successful source）保持原返回值（return value），没有新增错误包装（error wrapper）。
// @trace 1
#[test]
fn test_success_preserves_monotonic_source_result() {
    assert_eq!(measure(&Source { fail: false }).unwrap(), Duration::ZERO);
}

/// 失败来源（failing source）的原错误、标准来源、上下文和附件（original error/standard source/context/attachments）均可读取。
// @trace 2
#[test]
fn test_derived_source_survives_the_public_report_chain() {
    let report: Error = measure(&Source { fail: true }).unwrap_err();
    assert_eq!(report.current_kind(), &ErrorKind::Unavailable);
    assert!(report.contains_context::<MeasureContext>());
    let original = report.downcast_ref::<SampleError>().unwrap();
    assert_eq!(original.to_string(), "sample failed: sample denied");
    let source = original.source().unwrap().downcast_ref::<std::io::Error>().unwrap();
    assert!(std::ptr::eq(source, &original.source));
    assert_eq!(source.kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(report.downcast_ref::<SampleEvidence>(), Some(&SampleEvidence(17)));
    assert_eq!(report.attachment_ref::<Operation>(), Some(&Operation("sample_start")));
    assert!(report.frames().any(|frame| frame.downcast_ref::<ErrorKind>() == Some(&ErrorKind::Dependency)));
}
