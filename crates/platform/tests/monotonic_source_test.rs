// 原生公开来源测试（native public-source tests）：验证真实准备（real preparation）、采样（sampling）
// 及跨句柄间隔（cross-handle elapsed calculation），不测物理准确度（physical accuracy）或固定耗时（fixed latency）。
#![cfg(any(target_os = "windows", target_os = "linux", target_os = "android"))]

use pi_clock_core::source::{MonotonicSource, SuspendBehavior};
use pi_clock_core::std_time::Duration;
use pi_clock_platform::monotonic::PlatformMonotonicSource;

// 被测公开入口（public entries）：try_prepare／try_sample／try_elapsed。
// 仅在新测试进程（fresh test process）中单独筛选执行本项时，BASIS保证初始未发布（initially unpublished）；
// 整套测试（test suite）执行时其他用例（cases）可能已发布，不把屏障（barrier）当作首次发布（first publication）证明。
// 两个必要线程（necessary threads）经屏障（barrier）同时准备（prepare）并真实采样（sample）。
// 只比较各线程（threads）自身的先后采样（ordered sampling），不把跨线程一计数（one-tick）不确定性当顺序保证。
// @trace 1
#[test]
fn test_public_source_prepares_concurrently_and_samples_across_threads() {
    fn require_send_sync<T: Send + Sync>() {}
    require_send_sync::<PlatformMonotonicSource>();
    require_send_sync::<pi_clock_platform::monotonic::PlatformMonotonicSample>();
    let barrier = std::sync::Barrier::new(3);
    let samples = std::thread::scope(|scope| {
        let sample_pair = || {
            barrier.wait();
            let source = PlatformMonotonicSource::try_prepare().expect("并发准备（concurrent preparation）");
            let first = source.try_sample().unwrap();
            let second = source.try_sample().unwrap();
            (source, first, second)
        };
        let first = scope.spawn(sample_pair);
        let second = scope.spawn(sample_pair);
        barrier.wait();
        [first.join().unwrap(), second.join().unwrap()]
    });
    let source = PlatformMonotonicSource::try_prepare().unwrap();
    for (other, first, last) in samples {
        assert_eq!(source.try_elapsed(&first, &last).unwrap(), other.try_elapsed(&first, &last).unwrap());
        assert_eq!(source.try_elapsed(&first, &first).unwrap(), Duration::ZERO);
        assert_eq!(source.capabilities(), other.capabilities());
    }
}

// 被测公开入口（public entries）：try_prepare／try_sample／try_elapsed。
// 原句柄（original handle）全部离开作用域（scope）后，只持有旧样本（old sample）仍可重新准备（prepare）
// 并计算其零间隔（zero elapsed duration）；Copy无析构（destructor）但词法生命周期（lexical lifetime）已结束。
// @trace 2
#[test]
fn test_public_sample_outlives_all_original_source_handles() {
    let earlier = {
        let source = PlatformMonotonicSource::try_prepare().unwrap();
        source.try_sample().unwrap()
    };
    let source = PlatformMonotonicSource::try_prepare().unwrap();
    assert_eq!(source.try_elapsed(&earlier, &earlier).unwrap(), Duration::ZERO);
    let later = source.try_sample().unwrap();
    source.try_elapsed(&earlier, &later).expect("旧样本（old sample）与新句柄（new handle）相容（compatible）");
}

// 被测公开入口（public entries）：try_prepare／try_sample／try_elapsed／capabilities。
// 旧样本（old samples）越过句柄局部作用域（handle scope）后仍有效；两次独立准备（independent preparations）
// 的句柄（handles）可以组合计算，不用地址（addresses）或相同裸值（raw values）认证同域（same domain）。
// @trace 3
#[test]
fn test_public_source_reuses_basis_and_preserves_samples_after_handle_scope() {
    let (earlier, shared) = {
        let source = PlatformMonotonicSource::try_prepare().expect("真实来源准备（real source preparation）");
        let shared = source;
        (source.try_sample().expect("首次真实采样（first real sample）"), shared)
    };
    let source = PlatformMonotonicSource::try_prepare().expect("重复准备（repeated preparation）");
    let later = shared.try_sample().expect("复制句柄（copied handle）真实采样（real sample）");
    let elapsed = source.try_elapsed(&earlier, &later).expect("跨句柄间隔（cross-handle elapsed duration）");
    assert_eq!(shared.try_elapsed(&earlier, &later).unwrap(), elapsed);
    assert_eq!(source.try_elapsed(&earlier, &earlier).unwrap(), Duration::ZERO);
    assert_eq!(source.try_elapsed(&later, &later).unwrap(), Duration::ZERO);
    assert_eq!(source.capabilities(), shared.capabilities());
    #[cfg(target_os = "windows")]
    assert_eq!(source.capabilities().suspend, SuspendBehavior::Includes);
    #[cfg(any(target_os = "linux", target_os = "android"))]
    assert_eq!(source.capabilities().suspend, SuspendBehavior::Excludes);
}
