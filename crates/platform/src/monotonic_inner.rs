// 当前文件与配对声明共享同一个 Rust 模块（module）。
// 类型（types）、字段（fields）、唯一发布位置（unique publication site）及内部出口（internal entries）
// 由配对声明提供；这里落实局部操作（local operations），不导出内部状态（internal state）或原始载荷（raw payload）。

fn source_prepare_inner() -> Result<PlatformMonotonicSource> {
    // 探测（probe）只验证来源（source）可用，不包装公开样本（public sample）或保存时钟起点（clock origin）。
    // 静态槽（static slot）的成功借用（successful borrow）才可进入公开句柄（public handle）。
    let basis = prepare_once(&BASIS, prepare_basis, |candidate| {
        sample_raw(candidate)?;
        Ok(())
    })?;
    Ok(PlatformMonotonicSource { basis })
}

fn source_sample_inner(this: &PlatformMonotonicSource) -> Result<PlatformMonotonicSample> {
    let raw = sample_raw(this.basis)?;
    Ok(PlatformMonotonicSample { raw })
}

fn source_elapsed_inner(
    this: &PlatformMonotonicSource,
    earlier: &PlatformMonotonicSample,
    later: &PlatformMonotonicSample,
) -> Result<Duration> {
    elapsed_raw(this.basis, &earlier.raw, &later.raw)
}

fn source_capabilities_inner(this: &PlatformMonotonicSource) -> SourceCapabilities {
    this.basis.capabilities
}

fn prepare_once_inner<'a, F, P>(
    slot: &'a OnceLock<PreparedBasis>,
    build: F,
    probe: P,
) -> Result<&'a PreparedBasis>
where
    F: FnOnce() -> Result<PreparedBasis>,
    P: FnOnce(&PreparedBasis) -> Result<()>,
{
    if let Some(ready) = slot.get() {
        return Ok(ready);
    }

    // 构造（build）与探测（probe）均在发布同步（publication synchronization）之外；
    // 原错误（original error）直接传播，问号（?）退出时释放候选（candidate）。
    let candidate = build()?;
    probe(&candidate)?;
    if let Err(loser) = slot.set(candidate) {
        // 提交败者（publication loser）释放自己的候选（candidate），不替换胜者（winner）。
        drop(loser);
    }
    // OnceLock::set返回即保证有值；异常缺失仍明确报告内部错误（internal error）。
    slot.get().ok_or_else(|| {
        pi_result::Error::new(pi_result::ErrorKind::Internal)
            .attach("OnceLock::set完成后缺少已发布基准（published basis）")
    })
}

fn prepare_basis_inner() -> Result<PreparedBasis> {
    #[cfg(target_os = "windows")]
    {
        let mut raw_frequency = 0_i64;
        // 安全（SAFETY）：输出（output）为本次调用（call）独占、对齐且有效的i64存储；
        // QPF只在同步调用（synchronous call）期间写入，不保留指针（pointer）。
        let status = unsafe {
            windows_sys::Win32::System::Performance::QueryPerformanceFrequency(&mut raw_frequency)
        };
        if status == 0 {
            // QPF未承诺设置GetLastError；只保留实际返回状态（return status）。
            return Err(pi_result::Error::new(pi_result::ErrorKind::Dependency)
                .attach("QueryPerformanceFrequency返回失败状态（failure status）")
                .attach_opaque(status));
        }
        if raw_frequency <= 0 {
            return Err(pi_result::Error::new(pi_result::ErrorKind::Dependency)
                .attach("QueryPerformanceFrequency返回非正频率（nonpositive frequency）")
                .attach_opaque(raw_frequency));
        }
        // 正值检查（positive-value check）保证转换（conversion）无损；保存真实频率（frequency），
        // 而非先求近似的每计数纳秒因子（approximate nanoseconds-per-tick factor）。
        let frequency = NonZeroU64::new(raw_frequency as u64).ok_or_else(|| {
            pi_result::Error::new(pi_result::ErrorKind::Dependency)
                .attach("QueryPerformanceFrequency缺少正频率（positive frequency）")
        })?;
        Ok(PreparedBasis {
            capabilities: SourceCapabilities {
                suspend: pi_clock_core::source::SuspendBehavior::Includes,
            },
            frequency,
        })
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        Ok(PreparedBasis {
            capabilities: SourceCapabilities {
                suspend: pi_clock_core::source::SuspendBehavior::Excludes,
            },
        })
    }
    #[cfg(all(target_os = "wasi", any(target_env = "p1", target_env = "p2")))]
    {
        Ok(PreparedBasis {
            capabilities: SourceCapabilities {
                suspend: pi_clock_core::source::SuspendBehavior::Unspecified,
            },
        })
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics")))]
    {
        use wasm_bindgen::{JsCast, JsValue};

        let global = js_sys::global();
        let performance = js_sys::Reflect::get(&global, &JsValue::from_str("performance"))
            .map_err(|exception| {
                pi_result::Error::new(pi_result::ErrorKind::Dependency)
                    .attach("读取全局performance属性（global performance property）时抛出异常（exception）")
                    .attach_opaque(exception)
            })?;
        if !performance.is_object() {
            return Err(pi_result::Error::new(pi_result::ErrorKind::Unsupported)
                .attach("缺少合格的performance对象（performance object）"));
        }
        let now = js_sys::Reflect::get(&performance, &JsValue::from_str("now"))
            .map_err(|exception| {
                pi_result::Error::new(pi_result::ErrorKind::Dependency)
                    .attach("读取performance.now属性（property）时抛出异常（exception）")
                    .attach_opaque(exception)
            })?;
        let now = now.dyn_into::<js_sys::Function>().map_err(|_| {
            pi_result::Error::new(pi_result::ErrorKind::Unsupported)
                .attach("performance.now不是函数（function）")
        })?;
        // 只保存接收对象（receiver）和函数（function）；可信固定域（trusted fixed realm）
        // 是已定宿主前提（host precondition），对象形状（object shape）检查不认证该前提。
        Ok(PreparedBasis {
            capabilities: SourceCapabilities {
                suspend: pi_clock_core::source::SuspendBehavior::Unspecified,
            },
            performance,
            now,
        })
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "android",
        all(target_os = "wasi", any(target_env = "p1", target_env = "p2")),
        all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics")),
    )))]
    {
        Err(pi_result::Error::new(pi_result::ErrorKind::Unsupported)
            .attach("当前目标及功能组合缺少普通单调来源（ordinary monotonic source）"))
    }
}

fn sample_raw_inner(basis: &PreparedBasis) -> Result<RawSample> {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics"))))]
    let _ = basis;

    #[cfg(target_os = "windows")]
    {
        let mut counter = 0_i64;
        // 安全（SAFETY）：输出（output）独占借用本地有效i64存储；QPC同步写入且不保留
        // 指针（pointer），只在BOOL成功后读取输出（output），不读取未承诺的GetLastError。
        let status = unsafe {
            windows_sys::Win32::System::Performance::QueryPerformanceCounter(&mut counter)
        };
        if status == 0 {
            return Err(pi_result::Error::new(pi_result::ErrorKind::Dependency)
                .attach("QueryPerformanceCounter返回失败状态（failure status）")
                .attach_opaque(status));
        }
        if counter < 0 {
            return Err(pi_result::Error::new(pi_result::ErrorKind::Dependency)
                .attach("QueryPerformanceCounter返回负计数（negative counter）")
                .attach_opaque(counter));
        }
        Ok(RawSample::Windows(counter))
    }
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        use rustix::time::{clock_gettime_dynamic, ClockId, DynamicClockId};

        let value = clock_gettime_dynamic(DynamicClockId::Known(ClockId::Monotonic))
            .map_err(|cause| {
                pi_result::Error::new(pi_result::ErrorKind::Dependency)
                    .attach("clock_gettime_dynamic(CLOCK_MONOTONIC)失败（failure）")
                    .attach_opaque(cause)
            })?;
        if value.tv_sec < 0 || !(0..1_000_000_000).contains(&value.tv_nsec) {
            return Err(pi_result::Error::new(pi_result::ErrorKind::Dependency)
                .attach("CLOCK_MONOTONIC返回无效秒或纳秒字段（invalid seconds or nanoseconds）")
                .attach_opaque(value));
        }
        Ok(RawSample::Unix {
            seconds: value.tv_sec,
            nanoseconds: value.tv_nsec as u32,
        })
    }
    #[cfg(all(target_os = "wasi", target_env = "p1"))]
    {
        // 安全（SAFETY）：合格宿主（qualified host）履行WASI1 ABI并在成功时初始化绑定（binding）
        // 提供的有效输出存储（output storage）；precision=1仅为请求，不是准确度（accuracy）保证。
        let counter = unsafe { wasip1::clock_time_get(wasip1::CLOCKID_MONOTONIC, 1) }
            .map_err(|cause| {
                pi_result::Error::new(pi_result::ErrorKind::Dependency)
                    .attach("WASI1 clock_time_get(MONOTONIC)失败（failure）")
                    .attach_opaque(cause)
            })?;
        Ok(RawSample::WasiPreview1(counter))
    }
    #[cfg(all(target_os = "wasi", target_env = "p2"))]
    {
        // 导入缺失（missing import）与宿主陷阱（host trap）不伪装为可捕获的普通错误（ordinary error）。
        Ok(RawSample::WasiPreview2(wasip2::clocks::monotonic_clock::now()))
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics")))]
    {
        let value: wasm_bindgen::JsValue = basis.now.call0(&basis.performance)
            .map_err(|exception| {
                pi_result::Error::new(pi_result::ErrorKind::Dependency)
                    .attach("调用已保存performance.now函数（saved function）时抛出异常（exception）")
                    .attach_opaque(exception)
            })?;
        let milliseconds = value.as_f64().filter(|value| value.is_finite() && *value >= 0.0)
            .ok_or_else(|| {
                // 不格式化JavaScript值（value），避免错误处理（error handling）再次执行外部代码（external code）。
                pi_result::Error::new(pi_result::ErrorKind::Dependency)
                    .attach("performance.now未返回有限非负毫秒数（finite nonnegative milliseconds）")
                    .attach_opaque(value)
            })?;
        Ok(RawSample::Browser(milliseconds))
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "android",
        all(target_os = "wasi", any(target_env = "p1", target_env = "p2")),
        all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics")),
    )))]
    {
        // 不支持配置（unsupported configuration）没有可安全构造的RawSample，生产准备（preparation）先拒绝。
        Err(pi_result::Error::new(pi_result::ErrorKind::Unsupported)
            .attach("当前配置缺少原始单调采样（raw monotonic sampling）"))
    }
}

fn elapsed_raw_inner(
    basis: &PreparedBasis,
    earlier: &RawSample,
    later: &RawSample,
) -> Result<Duration> {
    #[cfg(not(target_os = "windows"))]
    let _ = basis;

    #[cfg(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "android",
        all(target_os = "wasi", any(target_env = "p1", target_env = "p2")),
        all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics")),
    ))]
    {
        let invalid = |reason: &'static str| {
            pi_result::Error::new(pi_result::ErrorKind::InvalidInput).attach(reason)
        };
        // 每个合法编译目标（compile target）只保留一种原始载荷（raw payload）；
        // 不透明公开构造（opaque public construction）及固定基准（fixed basis）保证同域（same domain）。
        let total_nanoseconds = match (earlier, later) {
            #[cfg(target_os = "windows")]
            (RawSample::Windows(first), RawSample::Windows(last)) => {
                if *first < 0 || *last < 0 || last < first {
                    return Err(invalid("QPC间隔载荷（elapsed payload）为负或逆序（reverse order）"));
                }
                // 先扩宽（widen）再求原始差（raw difference）；不分别量化端点（quantize endpoints）。
                let ticks = u128::try_from(i128::from(*last) - i128::from(*first))
                    .map_err(|_| invalid("QPC原始差（raw difference）不能表示为非负整数（nonnegative integer）"))?;
                ticks.checked_mul(1_000_000_000)
                    .and_then(|scaled| scaled.checked_div(u128::from(basis.frequency.get())))
                    .ok_or_else(|| invalid("QPC间隔换算（elapsed conversion）超出宽整数范围（wide-integer range）"))?
            }
            #[cfg(any(target_os = "linux", target_os = "android"))]
            (
                RawSample::Unix { seconds: first_seconds, nanoseconds: first_nanos },
                RawSample::Unix { seconds: last_seconds, nanoseconds: last_nanos },
            ) => {
                if *first_seconds < 0 || *last_seconds < 0
                    || *first_nanos >= 1_000_000_000 || *last_nanos >= 1_000_000_000
                {
                    return Err(invalid("Unix间隔（elapsed duration）含无效秒或纳秒字段（invalid seconds or nanoseconds）"));
                }
                // i64非负秒（nonnegative seconds）乘十亿仍在i128内；纳秒借位（nanosecond borrow）
                // 随直接差（direct difference）一并计算，不能分字段饱和（saturate）。
                let delta = (i128::from(*last_seconds) - i128::from(*first_seconds)) * 1_000_000_000
                    + i128::from(*last_nanos) - i128::from(*first_nanos);
                u128::try_from(delta).map_err(|_| invalid("Unix样本（samples）逆序（reverse order）"))?
            }
            #[cfg(all(target_os = "wasi", target_env = "p1"))]
            (RawSample::WasiPreview1(first), RawSample::WasiPreview1(last)) => {
                u128::from(last.checked_sub(*first)
                    .ok_or_else(|| invalid("WASI1样本（samples）逆序（reverse order）"))?)
            }
            #[cfg(all(target_os = "wasi", target_env = "p2"))]
            (RawSample::WasiPreview2(first), RawSample::WasiPreview2(last)) => {
                u128::from(last.checked_sub(*first)
                    .ok_or_else(|| invalid("WASI2样本（samples）逆序（reverse order）"))?)
            }
            #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics")))]
            (RawSample::Browser(first), RawSample::Browser(last)) => {
                if !first.is_finite() || !last.is_finite() || *first < 0.0 || *last < 0.0 || last < first {
                    return Err(invalid("浏览器样本（browser samples）非有限、负值或逆序（nonfinite, negative or reversed）"));
                }
                let nanoseconds = ((*last - *first) * 1_000_000.0).floor();
                // 此常量（constant）是可精确表示的2^128，严格小于（strictly less than）
                // 才能转换（convert）；u128::MAX as f64会舍入（round）到2^128，不能用作含端点上界（inclusive bound）。
                const U128_EXCLUSIVE_BOUND: f64 = 340_282_366_920_938_463_463_374_607_431_768_211_456.0;
                if !nanoseconds.is_finite() || nanoseconds < 0.0 || nanoseconds >= U128_EXCLUSIVE_BOUND {
                    return Err(invalid("浏览器纳秒差（browser nanosecond delta）超出整数转换范围（integer-conversion range）"));
                }
                nanoseconds as u128
            }
        };
        // 保留完整Duration范围（range），不提前缩成u64总纳秒（total nanoseconds）。
        let seconds = u64::try_from(total_nanoseconds / 1_000_000_000)
            .map_err(|_| invalid("间隔（elapsed duration）超出Duration秒数范围（seconds range）"))?;
        let nanoseconds = (total_nanoseconds % 1_000_000_000) as u32;
        Ok(Duration::new(seconds, nanoseconds))
    }
    #[cfg(not(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "android",
        all(target_os = "wasi", any(target_env = "p1", target_env = "p2")),
        all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics")),
    )))]
    {
        let _ = (earlier, later);
        Err(pi_result::Error::new(pi_result::ErrorKind::InvalidInput)
            .attach("当前配置没有相容原始载荷（compatible raw payload）"))
    }
}

// @inline-test-exception：局部发布协议（local publication protocol）与纯算术（pure arithmetic）
// 经既有私有壳入口（private shell entries）验证，不建立公开来源（public source）或生产状态（production state）。
#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;
    use pi_clock_core::source::SuspendBehavior;

    // 局部候选（local candidate）只为协议测试（protocol tests）提供合法固定元数据（fixed metadata）。
    fn candidate(frequency: u64) -> PreparedBasis {
        PreparedBasis {
            capabilities: SourceCapabilities { suspend: SuspendBehavior::Includes },
            frequency: NonZeroU64::new(frequency).expect("测试频率（test frequency）必须为正"),
        }
    }

    // 被测入口（entry）：prepare_once，经真实prepare_basis构造（build）当前宿主（host）元数据（metadata）。
    // Windows频率（frequency）必须为正且休眠能力（suspend capability）为Includes；本项不取样（sample）。
    // @trace 1
    #[test]
    fn test_prepare_basis_supplies_positive_windows_frequency() {
        let slot = OnceLock::new();
        let prepared = prepare_once(&slot, prepare_basis, |basis| {
            assert!(basis.frequency.get() > 0);
            assert_eq!(basis.capabilities.suspend, SuspendBehavior::Includes);
            Ok(())
        }).expect("真实基准准备（real basis preparation）应成功");
        assert_eq!(prepared.capabilities.suspend, SuspendBehavior::Includes);
    }

    // 被测入口（entry）：prepare_once。真实探测（real probe）调用sample_raw，
    // 要求QPC原始计数（raw counter）非负且同线程连续读取（sequential reads）有序；不要求固定精度（precision）。
    // @trace 2
    #[test]
    fn test_sample_raw_probe_reads_ordered_nonnegative_windows_counts() {
        let slot = OnceLock::new();
        prepare_once(&slot, prepare_basis, |basis| {
            let RawSample::Windows(first) = sample_raw(basis)?;
            let RawSample::Windows(second) = sample_raw(basis)?;
            assert!(first >= 0);
            assert!(second >= first);
            assert!(slot.get().is_none());
            Ok(())
        }).expect("真实采样探测（real sampling probe）应成功");
    }

    // 被测入口（entry）：elapsed_raw。频率（frequency）为3，原始端点（raw endpoints）1与3，
    // 直接差（direct delta）2/3秒须向下取整（floor）为666666666纳秒；端点先换算会差1纳秒。
    // @trace 3
    #[test]
    fn test_elapsed_raw_windows_rounds_the_direct_delta() {
        assert_eq!(elapsed_raw(&candidate(3), &RawSample::Windows(1), &RawSample::Windows(3)).unwrap(),
            Duration::new(0, 666_666_666));
    }

    // 被测入口（entry）：elapsed_raw。有效相同计数（equal valid counters）包括零和i64上界（upper bound），
    // 均得到零时长（zero duration），不依赖真实时间（real time）或系统频率（system frequency）。
    // @trace 4
    #[test]
    fn test_elapsed_raw_windows_equal_valid_values_are_zero() {
        let basis = candidate(i64::MAX as u64);
        for value in [0, 1, i64::MAX] {
            let sample = RawSample::Windows(value);
            assert_eq!(elapsed_raw(&basis, &sample, &sample).unwrap(), Duration::ZERO);
        }
    }

    // 被测入口（entry）：elapsed_raw。负端点（negative endpoints）即使相等也非法，
    // 非负逆序（nonnegative reverse order）必须为InvalidInput，不能归零（clamp to zero）。
    // @trace 5
    #[test]
    fn test_elapsed_raw_windows_rejects_negative_or_reversed_values() {
        let basis = candidate(1);
        for (first, last) in [(-1, -1), (-1, 0), (0, -1), (2, 1), (i64::MIN, i64::MAX)] {
            let error = elapsed_raw(&basis, &RawSample::Windows(first), &RawSample::Windows(last))
                .expect_err("非法端点（invalid endpoints）应被拒绝");
            assert_eq!(error.current_context(), &pi_result::ErrorKind::InvalidInput);
        }
    }

    // 被测入口（entry）：elapsed_raw。用独立算例（independent examples）检查跨秒（second boundary）
    // 和不足一纳秒（subnanosecond）的向下取整（round down），不从实现（implementation）重算期望值。
    // @trace 6
    #[test]
    fn test_elapsed_raw_windows_normalizes_seconds_and_floors_subnanoseconds() {
        let cases = [
            (10, 9, 21, Duration::new(1, 200_000_000)),
            (3_000_000_000, 0, 1, Duration::ZERO),
            (3_000_000_000, 0, 3, Duration::new(0, 1)),
            (2_000_000_000, 0, 3, Duration::new(0, 1)),
        ];
        for (frequency, first, last, expected) in cases {
            assert_eq!(
                elapsed_raw(&candidate(frequency), &RawSample::Windows(first), &RawSample::Windows(last)).unwrap(),
                expected,
            );
        }
    }

    // 被测入口（entry）：elapsed_raw。最大非负i64计数（maximum nonnegative i64 counter）
    // 与靠近上界（upper bound）的相邻值验证扩宽求差（widened difference）；Duration不能缩成u64总纳秒。
    // @trace 7
    #[test]
    fn test_elapsed_raw_windows_preserves_wide_duration_and_extreme_counts() {
        let cases = [
            (1, 0, i64::MAX, Duration::new(i64::MAX as u64, 0)),
            (1, i64::MAX - 1, i64::MAX, Duration::new(1, 0)),
            (1_000_000_000, i64::MAX - 1, i64::MAX, Duration::new(0, 1)),
            (i64::MAX as u64, 0, i64::MAX, Duration::new(1, 0)),
            (1, 0, 18_446_744_074, Duration::new(18_446_744_074, 0)),
        ];
        for (frequency, first, last, expected) in cases {
            assert_eq!(
                elapsed_raw(&candidate(frequency), &RawSample::Windows(first), &RawSample::Windows(last)).unwrap(),
                expected,
            );
        }
    }

    // 被测入口（entry）：prepare_once。首次成功发布（publication）后，重复调用（repeated call）
    // 必须借用同一值（same value），且两个未调用闭包（uncalled closures）不能执行。
    // @trace 8
    #[test]
    fn test_prepare_once_publishes_and_reuses_ready_basis() {
        let slot = OnceLock::new();
        let calls = std::cell::Cell::new((0, 0));
        let first = prepare_once(
            &slot,
            || { calls.set((1, 0)); Ok(candidate(7)) },
            |basis| {
                assert_eq!(basis.frequency.get(), 7);
                assert!(slot.get().is_none());
                calls.set((1, 1));
                Ok(())
            },
        ).expect("首次发布（first publication）应成功");
        let second = prepare_once(
            &slot,
            || panic!("已就绪（ready）不得重新构造（build）"),
            |_| panic!("已就绪（ready）不得重新探测（probe）"),
        ).expect("重复准备（repeated preparation）应成功");
        assert!(std::ptr::eq(first, second));
        assert!(std::ptr::eq(first, slot.get().expect("已发布值（published value）")));
        assert_eq!(calls.get(), (1, 1));
    }

    // 被测入口（entry）：prepare_once。构造失败（build failure）原样传播附件（attachment），
    // 不探测（probe）或写槽（slot）；相同空槽（empty slot）随后可成功重试（retry）。
    // @trace 9
    #[test]
    fn test_prepare_once_build_failure_preserves_error_and_allows_retry() {
        let slot = OnceLock::new();
        let marker = std::sync::Arc::new(23_u32);
        let failure = pi_result::Error::new(pi_result::ErrorKind::Dependency)
            .attach_opaque(marker.clone());
        let error = prepare_once(&slot, || Err(failure), |_| panic!("构造失败不得探测（probe）"))
            .err().expect("应返回原错误（original error）");
        assert_eq!(error.current_context(), &pi_result::ErrorKind::Dependency);
        assert!(std::sync::Arc::ptr_eq(error.downcast_ref::<std::sync::Arc<u32>>().unwrap(), &marker));
        assert!(slot.get().is_none());
        assert_eq!(prepare_once(&slot, || Ok(candidate(11)), |_| Ok(())).unwrap().frequency.get(), 11);
    }

    // 被测入口（entry）：prepare_once。探测失败（probe failure）不发布候选（candidate），
    // 保留原错误（original error）附件（attachment）并允许后续重试（retry）。
    // @trace 10
    #[test]
    fn test_prepare_once_probe_failure_preserves_error_and_allows_retry() {
        let slot = OnceLock::new();
        let failure = pi_result::Error::new(pi_result::ErrorKind::Unavailable).attach_opaque(29_u32);
        let error = prepare_once(&slot, || Ok(candidate(3)), |_| Err(failure))
            .err().expect("探测失败（probe failure）应返回错误（error）");
        assert_eq!(error.current_context(), &pi_result::ErrorKind::Unavailable);
        assert_eq!(error.downcast_ref::<u32>(), Some(&29));
        assert!(slot.get().is_none());
        assert_eq!(prepare_once(&slot, || Ok(candidate(13)), |_| Ok(())).unwrap().frequency.get(), 13);
    }

    // 被测入口（entry）：prepare_once。通道（channels）强制两次探测（probes）均进入后，
    // 才允许甲发布（publish），乙随后提交失败并返回甲的同一借用（borrow）。
    // 有界等待（bounded waits）只控制测试时序（test interleaving），不代补生产发布（publication）。
    // @trace 11
    #[test]
    fn test_prepare_once_concurrent_loser_reuses_the_published_winner() {
        let slot = OnceLock::new();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (a_tx, a_rx) = std::sync::mpsc::channel();
        let (b_tx, b_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let slot = &slot;
            let entered = entered_tx.clone();
            let a = scope.spawn(move || prepare_once(slot, || Ok(candidate(17)), |_| {
                entered.send(()).unwrap();
                a_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            }));
            let b = scope.spawn(move || prepare_once(slot, || Ok(candidate(19)), |_| {
                entered_tx.send(()).unwrap();
                b_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Ok(())
            }));
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(slot.get().is_none());
            a_tx.send(()).unwrap();
            let winner = a.join().unwrap().unwrap();
            b_tx.send(()).unwrap();
            let loser_result = b.join().unwrap().unwrap();
            assert_eq!(winner.frequency.get(), 17);
            assert!(std::ptr::eq(winner, loser_result));
            assert!(std::ptr::eq(winner, slot.get().unwrap()));
        });
    }

    // 被测入口（entry）：prepare_once。构造（build）与探测（probe）的单次错误（individual error）
    // 均可在另一调用（call）发布成功之后返回；不能回滚（rollback）他人的成功。
    // @trace 12
    #[test]
    fn test_prepare_once_failure_can_coexist_with_another_success() {
        for fail_during_build in [true, false] {
            let slot = OnceLock::new();
            let (entered_tx, entered_rx) = std::sync::mpsc::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            std::thread::scope(|scope| {
                let slot = &slot;
                let failed = scope.spawn(move || {
                    if fail_during_build {
                        prepare_once(&slot, || {
                            entered_tx.send(()).unwrap();
                            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                            Err(pi_result::Error::new(pi_result::ErrorKind::Dependency).attach_opaque(31_u32))
                        }, |_| panic!("不得探测（probe）失败候选（failed candidate）"))
                    } else {
                        prepare_once(&slot, || Ok(candidate(23)), |_| {
                            entered_tx.send(()).unwrap();
                            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                            Err(pi_result::Error::new(pi_result::ErrorKind::Dependency).attach_opaque(31_u32))
                        })
                    }
                });
                entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                let winner = prepare_once(&slot, || Ok(candidate(29)), |_| Ok(())).unwrap();
                release_tx.send(()).unwrap();
                let error = failed.join().unwrap().err().unwrap();
                assert_eq!(error.current_context(), &pi_result::ErrorKind::Dependency);
                assert_eq!(error.downcast_ref::<u32>(), Some(&31));
                assert!(std::ptr::eq(winner, slot.get().unwrap()));
                assert_eq!(winner.frequency.get(), 29);
            });
        }
    }

    // 被测入口（entry）：prepare_once。就绪快路（ready fast path）不能泄漏未调用闭包（uncalled closures）
    // 拥有的资源（owned resources）；用引用计数（reference counts）观察两份捕获（captures）释放。
    // @trace 13
    #[test]
    fn test_prepare_once_ready_path_releases_uncalled_closure_captures() {
        let slot = OnceLock::new();
        assert!(slot.set(candidate(37)).is_ok());
        let owned = std::sync::Arc::new(());
        let build_capture = owned.clone();
        let probe_capture = owned.clone();
        prepare_once(&slot, move || {
            drop(build_capture);
            panic!("不得重新构造（build）")
        }, move |_| {
            drop(probe_capture);
            panic!("不得重新探测（probe）")
        }).unwrap();
        assert_eq!(std::sync::Arc::strong_count(&owned), 1);
    }
}
