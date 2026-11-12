//! 公开时间点（public time point）的受检运算（checked arithmetic）验证。
//! 只消费公开接口（public APIs），无需全局安装（global installation）。

use pi_clock_core::std_time::Duration;
use pi_clock_manual::global::GlobalManualInstant;
use pi_result::ErrorKind;

// @trace 2
/// 验证标准时长（standard duration）无损返回及跨秒往返（cross-second round trip）。
#[test]
fn test_duration_since_origin_preserves_every_nanosecond() {
    assert_eq!(GlobalManualInstant::ZERO.duration_since_origin(), std::time::Duration::ZERO);
    assert_eq!(GlobalManualInstant::MAX.duration_since_origin(), std::time::Duration::new(18_446_744_073, 709_551_615));
    let duration: std::time::Duration = Duration::new(2, 17);
    let point = GlobalManualInstant::try_from_elapsed(duration).unwrap();
    assert_eq!(point.duration_since_origin(), std::time::Duration::new(2, 17));
    assert_eq!(GlobalManualInstant::try_from_elapsed(point.duration_since_origin()).unwrap(), point);
}


// @trace 3
/// 验证受检加法（checked addition）的恒等、跨秒及范围上界（range upper bound）。
#[test]
fn test_add_rejects_overflow_without_saturation() {
    let point = GlobalManualInstant::try_from_elapsed(Duration::new(0, 999_999_999)).unwrap();
    assert_eq!(point.try_add(Duration::ZERO).unwrap(), point);
    assert_eq!(GlobalManualInstant::MAX.try_add(Duration::ZERO).unwrap(), GlobalManualInstant::MAX);
    assert_eq!(point.try_add(Duration::from_nanos(2)).unwrap().duration_since_origin(), Duration::new(1, 1));
    assert_eq!(GlobalManualInstant::ZERO.try_add(Duration::new(18_446_744_073, 709_551_615)).unwrap(), GlobalManualInstant::MAX);
    for duration in [Duration::from_nanos(1), Duration::MAX] {
        assert_eq!(GlobalManualInstant::MAX.try_add(duration).unwrap_err().current_context(), &ErrorKind::InvalidInput);
    }
    assert_eq!(GlobalManualInstant::ZERO.try_add(Duration::MAX).unwrap_err().current_context(), &ErrorKind::InvalidInput);
    assert_eq!(point.duration_since_origin(), Duration::new(0, 999_999_999));
}

// @trace 4
/// 验证受检减法（checked subtraction）的零界、同值与完整时长（full duration）。
#[test]
fn test_sub_rejects_underflow_without_clamping() {
    let point = GlobalManualInstant::try_from_elapsed(Duration::new(1, 1)).unwrap();
    assert_eq!(GlobalManualInstant::ZERO.try_sub(Duration::ZERO).unwrap(), GlobalManualInstant::ZERO);
    assert_eq!(GlobalManualInstant::MAX.try_sub(Duration::ZERO).unwrap(), GlobalManualInstant::MAX);
    assert_eq!(point.try_sub(Duration::new(1, 1)).unwrap(), GlobalManualInstant::ZERO);
    assert_eq!(point.try_sub(Duration::from_nanos(2)).unwrap().duration_since_origin(), Duration::new(0, 999_999_999));
    assert_eq!(GlobalManualInstant::MAX.try_sub(Duration::new(18_446_744_073, 709_551_615)).unwrap(), GlobalManualInstant::ZERO);
    assert_eq!(GlobalManualInstant::ZERO.try_sub(Duration::from_nanos(1)).unwrap_err().current_context(), &ErrorKind::InvalidInput);
    assert_eq!(GlobalManualInstant::MAX.try_sub(Duration::MAX).unwrap_err().current_context(), &ErrorKind::InvalidInput);
    assert_eq!(point.duration_since_origin(), Duration::new(1, 1));
}

// @trace 5
/// 验证非负间隔（nonnegative interval）的相等、全域及逆序拒绝（reversed-order rejection）。
#[test]
fn test_duration_since_rejects_reversed_points() {
    assert_eq!(GlobalManualInstant::MAX.try_duration_since(GlobalManualInstant::ZERO).unwrap(), Duration::new(18_446_744_073, 709_551_615));
    assert_eq!(GlobalManualInstant::MAX.try_duration_since(GlobalManualInstant::MAX).unwrap(), Duration::ZERO);
    assert_eq!(GlobalManualInstant::ZERO.try_duration_since(GlobalManualInstant::ZERO).unwrap(), Duration::ZERO);
    assert_eq!(GlobalManualInstant::ZERO.try_duration_since(GlobalManualInstant::MAX).unwrap_err().current_context(), &ErrorKind::InvalidInput);
    let earlier = GlobalManualInstant::try_from_elapsed(Duration::new(0, 999_999_999)).unwrap();
    let later = GlobalManualInstant::try_from_elapsed(Duration::new(1, 1)).unwrap();
    assert_eq!(later.try_duration_since(earlier).unwrap(), Duration::from_nanos(2));
}

// @trace 1
/// 验证显式绑定（explicit binding）的零、最大值与超界（range overflow）；
/// 最大值加一使用独立十进制时长（duration literal），不依赖被测转换（conversion）。
#[test]
fn test_elapsed_binding_checks_the_full_duration_range() {
    assert_eq!(GlobalManualInstant::try_from_elapsed(Duration::ZERO).unwrap(), GlobalManualInstant::ZERO);
    assert_eq!(GlobalManualInstant::try_from_elapsed(Duration::new(18_446_744_073, 709_551_615)).unwrap(), GlobalManualInstant::MAX);
    for duration in [Duration::new(18_446_744_073, 709_551_616), Duration::MAX] {
        let error = GlobalManualInstant::try_from_elapsed(duration).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::InvalidInput);
    }
}
