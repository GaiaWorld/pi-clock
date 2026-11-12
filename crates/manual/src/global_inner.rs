// 配对实现入口（paired implementation entries）；与global.rs属于同一实际模块（actual module）。

// 内部用途（internal purpose）：instant_from_elapsed_inner落实相应声明（declaration）的局部行为。
fn instant_from_elapsed_inner(elapsed: Duration) -> Result<GlobalManualInstant>
{
    let nanos = u64::try_from(elapsed.as_nanos()).map_err(|_| {
        pi_result::Error::new(pi_result::ErrorKind::InvalidInput)
            .attach("经过时长超出时间点范围（elapsed duration exceeds time-point range）")
    })?;
    Ok(GlobalManualInstant(nanos))
}

// 内部用途（internal purpose）：instant_duration_inner落实相应声明（declaration）的局部行为。
fn instant_duration_inner(value: GlobalManualInstant) -> Duration
{
    Duration::from_nanos(value.0)
}

// 内部用途（internal purpose）：instant_add_inner落实相应声明（declaration）的局部行为。
fn instant_add_inner(value: GlobalManualInstant, duration: Duration) -> Result<GlobalManualInstant>
{
    // 标准时长（standard duration）的纳秒上限加u64仍在u128内；缩窄（narrowing）单独受检。
    let nanos = u64::try_from(u128::from(value.0) + duration.as_nanos()).map_err(|_| {
        pi_result::Error::new(pi_result::ErrorKind::InvalidInput)
            .attach("时间点加法超界（time-point addition overflow）")
    })?;
    Ok(GlobalManualInstant(nanos))
}

// 内部用途（internal purpose）：instant_sub_inner落实相应声明（declaration）的局部行为。
fn instant_sub_inner(value: GlobalManualInstant, duration: Duration) -> Result<GlobalManualInstant>
{
    let nanos = u128::from(value.0).checked_sub(duration.as_nanos()).ok_or_else(|| {
        pi_result::Error::new(pi_result::ErrorKind::InvalidInput)
            .attach("时间点减法下溢（time-point subtraction underflow）")
    })?;
    // 差值（difference）不大于原u64值，转换（conversion）无损。
    Ok(GlobalManualInstant(nanos as u64))
}

// 内部用途（internal purpose）：instant_since_inner落实相应声明（declaration）的局部行为。
fn instant_since_inner(value: GlobalManualInstant, earlier: GlobalManualInstant) -> Result<Duration>
{
    let nanos = value.0.checked_sub(earlier.0).ok_or_else(|| {
        pi_result::Error::new(pi_result::ErrorKind::InvalidInput)
            .attach("时间点顺序相反（reversed time-point order）")
    })?;
    Ok(Duration::from_nanos(nanos))
}

// 内部用途（internal purpose）：init_global_inner落实相应声明（declaration）的局部行为。
fn init_global_inner() -> Result<()>
{
    try_install(&GLOBAL, PlatformMonotonicSource::try_prepare)
}

// 内部用途（internal purpose）：read_global_inner落实相应声明（declaration）的局部行为。
#[inline]
fn read_global_inner() -> Result<GlobalManualInstant>
{
    let state = GLOBAL.get().ok_or_else(|| {
        pi_result::Error::new(pi_result::ErrorKind::InvalidState)
            .attach("手推时钟尚未安装（manual clock is not installed）")
    })?;
    Ok(read_state(state))
}

// 内部用途（internal purpose）：refresh_global_inner落实相应声明（declaration）的局部行为。
fn refresh_global_inner() -> Result<RefreshOutcome>
{
    let state = GLOBAL.get().ok_or_else(|| {
        pi_result::Error::new(pi_result::ErrorKind::InvalidState)
            .attach("手推时钟尚未安装（manual clock is not installed）")
    })?;
    try_refresh_state(state)
}

// 内部用途（internal purpose）：capabilities_global_inner落实相应声明（declaration）的局部行为。
fn capabilities_global_inner() -> Result<SourceCapabilities>
{
    let state = GLOBAL.get().ok_or_else(|| {
        pi_result::Error::new(pi_result::ErrorKind::InvalidState)
            .attach("手推时钟尚未安装（manual clock is not installed）")
    })?;
    Ok(state.source.capabilities())
}

// 内部用途（internal purpose）：install_inner落实相应声明（declaration）的局部行为。
fn install_inner<S, F>(slot: &OnceLock<GlobalManualState<S>>, prepare: F) -> Result<()>
where
    S: MonotonicSource,
    F: FnOnce() -> Result<S>,
{
    if slot.get().is_some() {
        return Err(pi_result::Error::new(pi_result::ErrorKind::AlreadyExists)
            .attach("手推时钟已安装（manual clock already installed）"));
    }
    // 来源准备与首采样（source preparation and first sampling）不占用发布同步（publication synchronization）。
    let candidate = try_build_state(prepare()?)?;
    if let Err(loser) = slot.set(candidate) {
        // 竞争败者（losing contender）仅释放自己的候选（candidate），原实例（original instance）保留。
        drop(loser);
        return Err(pi_result::Error::new(pi_result::ErrorKind::AlreadyExists)
            .attach("另一调用已安装手推时钟（another call installed the manual clock）"));
    }
    Ok(())
}

// 内部用途（internal purpose）：build_state_inner落实相应声明（declaration）的局部行为。
fn build_state_inner<S: MonotonicSource>(source: S) -> Result<GlobalManualState<S>>
{
    let origin = source.try_sample()?;
    Ok(GlobalManualState { source, origin, cached_nanos: AtomicU64::new(0) })
}

// 内部用途（internal purpose）：read_state_inner落实相应声明（declaration）的局部行为。
#[inline]
fn read_state_inner<S: MonotonicSource>(state: &GlobalManualState<S>) -> GlobalManualInstant
{
    GlobalManualInstant(state.cached_nanos.load(std::sync::atomic::Ordering::SeqCst))
}

// 内部用途（internal purpose）：refresh_state_inner落实相应声明（declaration）的局部行为。
fn refresh_state_inner<S: MonotonicSource>(state: &GlobalManualState<S>) -> Result<RefreshOutcome>
{
    use std::sync::atomic::Ordering::SeqCst;

    let sample = state.source.try_sample()?;
    let elapsed = state.source.try_elapsed(&state.origin, &sample)?;
    let candidate = u64::try_from(elapsed.as_nanos()).map_err(|_| {
        pi_result::Error::new(pi_result::ErrorKind::ResourceExhausted)
            .attach("真实经过时长超出缓存范围（real elapsed duration exceeds cache range）")
    })?;
    // 重试（retry）只比较固定候选（fixed candidate）；来源调用（source calls）已完成且不会重做。
    let (published, advanced) = match state.cached_nanos.try_update(SeqCst, SeqCst, |old| {
        if candidate > old { Some(candidate) } else { None }
    }) {
        Ok(_) => (candidate, true),
        Err(observed) => (observed, false),
    };
    // 返回本次决定点（decision point）的值，不用后续加载（subsequent load）改变决定的含义。
    Ok(RefreshOutcome {
        sampled: GlobalManualInstant(candidate),
        published: GlobalManualInstant(published),
        advanced,
    })
}

#[cfg(test)]
mod tests {
    //! 私有局部槽（private local slot）仅调用同一生产协议（production protocol）。
    //! 受控来源（controlled source）只提供样本、错误与计数，不代补安装或发布（publication）。

    use super::*;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use pi_clock_core::source::SuspendBehavior;
    use pi_result::{Error, ErrorKind};

    /// 单线程受控来源（single-thread controlled source），成功样本（successful samples）由next设置且须非递减。
    /// 采样／间隔计数（sampling/elapsed counts）包含失败尝试；两个错误槽（error slots）各只消费一次。
    /// 释放计数（disposal count）观察实例析构（instance drop）；Rc／Cell还验证未增加线程约束（thread bounds）。
    struct ControlledSource {
        next: Cell<u64>,
        samples: Rc<Cell<usize>>,
        elapsed_calls: Rc<Cell<usize>>,
        drops: Rc<Cell<usize>>,
        sample_error: RefCell<Option<Error>>,
        elapsed_error: RefCell<Option<Error>>,
    }

    impl ControlledSource {
        /// 以任意u64纳秒样本（nanosecond sample）创建来源（source），计数清零且没有待返回错误（pending errors）。
        fn new(value: u64) -> Self {
            Self {
                next: Cell::new(value),
                samples: Rc::new(Cell::new(0)),
                elapsed_calls: Rc::new(Cell::new(0)),
                drops: Rc::new(Cell::new(0)),
                sample_error: RefCell::new(None),
                elapsed_error: RefCell::new(None),
            }
        }
    }

    impl Drop for ControlledSource {
        /// 仅累计本实例的释放（instance disposal），保留外部共享计数（shared count）供断言观察。
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }

    impl MonotonicSource for ControlledSource {
        type Sample = u64;

        /// 先累计尝试（attempt），再取走一次性错误（one-shot error）；成功时返回next，不自行推进（advance）。
        fn try_sample(&self) -> Result<u64> {
            self.samples.set(self.samples.get() + 1);
            if let Some(error) = self.sample_error.borrow_mut().take() {
                return Err(error);
            }
            Ok(self.next.get())
        }

        /// 先累计尝试并消费一次性错误（one-shot error），否则计算同源样本（same-source samples）的受检差（checked difference）。
        fn try_elapsed(&self, earlier: &u64, later: &u64) -> Result<Duration> {
            self.elapsed_calls.set(self.elapsed_calls.get() + 1);
            if let Some(error) = self.elapsed_error.borrow_mut().take() {
                return Err(error);
            }
            let nanos = later.checked_sub(*earlier).ok_or_else(|| Error::new(ErrorKind::InvalidInput))?;
            Ok(Duration::from_nanos(nanos))
        }

        /// 固定返回未指定的休眠语义（unspecified suspend behavior），不采样或修改计数（counts）。
        fn capabilities(&self) -> SourceCapabilities {
            SourceCapabilities { suspend: SuspendBehavior::Unspecified }
        }
    }

    // @trace 2
    /// 普通读取（ordinary read）只观察缓存（cache），来源变化与采样错误（sampling error）不影响读数。
    #[test]
    fn test_read_state_is_a_source_free_snapshot() {
        let state = try_build_state(ControlledSource::new(123)).unwrap();
        state.source.next.set(999);
        *state.source.sample_error.borrow_mut() = Some(Error::new(ErrorKind::Dependency));
        assert_eq!(read_state(&state), GlobalManualInstant::ZERO);
        assert_eq!(read_state(&state), GlobalManualInstant::ZERO);
        assert_eq!(state.source.samples.get(), 1);
        assert_eq!(state.source.elapsed_calls.get(), 0);
        assert!(state.source.sample_error.borrow().is_some());
        assert_eq!(state.origin, 123);
    }

    // @trace 3
    /// 安装协议（installation protocol）覆盖准备／首样本失败（preparation/first-sample failure）、
    /// 显式重试（explicit retry）、重复拒绝（duplicate rejection）及未执行闭包释放（unused closure disposal）。
    #[test]
    fn test_install_retries_failures_and_rejects_duplicate_without_preparing() {
        let slot = OnceLock::<GlobalManualState<ControlledSource>>::new();
        let error = try_install(&slot, || Err(Error::new(ErrorKind::Dependency).attach("prepare-marker").attach_opaque(41_u32))).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::Dependency);
        assert_eq!(error.downcast_ref::<u32>(), Some(&41));
        assert!(format!("{error:?}").contains("prepare-marker"));
        assert!(slot.get().is_none());

        let failed = ControlledSource::new(7);
        let failed_drops = failed.drops.clone();
        *failed.sample_error.borrow_mut() = Some(Error::new(ErrorKind::InvalidState).attach("origin-marker"));
        let error = try_install(&slot, || Ok(failed)).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::InvalidState);
        assert!(format!("{error:?}").contains("origin-marker"));
        assert_eq!(failed_drops.get(), 1);
        assert!(slot.get().is_none());

        let attempts = Cell::new(0);
        try_install(&slot, || {
            attempts.set(attempts.get() + 1);
            assert!(slot.get().is_none());
            Ok(ControlledSource::new(77))
        }).unwrap();
        let winner = slot.get().unwrap();
        assert_eq!(attempts.get(), 1);
        assert_eq!(winner.origin, 77);
        assert_eq!(read_state(winner), GlobalManualInstant::ZERO);

        let unused = ControlledSource::new(999);
        let unused_drops = unused.drops.clone();
        let error = try_install(&slot, || {
            attempts.set(attempts.get() + 1);
            Ok(unused)
        }).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::AlreadyExists);
        assert_eq!(attempts.get(), 1);
        assert_eq!(unused_drops.get(), 1);
        assert_eq!(slot.get().unwrap().origin, 77);
        assert_eq!(read_state(slot.get().unwrap()), GlobalManualInstant::ZERO);
    }

    /// 可共享来源（shareable source）只在首采样（first sampling）同步两个竞争候选（competing candidates）。
    struct RacingSource {
        origin: u64,
        gate: Option<std::sync::Arc<std::sync::Barrier>>,
        drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Drop for RacingSource {
        /// 每个候选实例（candidate instance）释放时原子累计一次，供胜败者生命周期（winner/loser lifecycle）断言使用。
        fn drop(&mut self) {
            self.drops.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl MonotonicSource for RacingSource {
        type Sample = u64;
        /// 本夹具（fixture）仅用于首采样竞争（first-sampling race）；有屏障（barrier）时每次调用都等待再返回固定样本（fixed sample）。
        fn try_sample(&self) -> Result<u64> {
            if let Some(gate) = &self.gate { gate.wait(); }
            Ok(self.origin)
        }
        /// 测试只提供同源有序样本（ordered same-source samples）；逆序触发夹具断言（fixture assertion）。
        fn try_elapsed(&self, earlier: &u64, later: &u64) -> Result<Duration> {
            Ok(Duration::from_nanos(later.checked_sub(*earlier).unwrap()))
        }
        /// 固定返回未指定的休眠语义（unspecified suspend behavior），不访问屏障（barrier）或计数。
        fn capabilities(&self) -> SourceCapabilities {
            SourceCapabilities { suspend: SuspendBehavior::Unspecified }
        }
    }

    // @trace 4
    /// 两个候选（candidates）均已观察空槽（empty slot）再竞争提交（publication），
    /// 只一个成功且败者立即释放（loser disposal）；没有重置（reset）。
    #[test]
    fn test_install_concurrent_candidates_have_one_winner_and_drop_loser() {
        use std::sync::{Arc, Barrier};
        use std::sync::atomic::{AtomicUsize, Ordering};
        let slot = OnceLock::new();
        let gate = Arc::new(Barrier::new(2));
        let drops = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            let first = scope.spawn(|| try_install(&slot, || Ok(RacingSource { origin: 10, gate: Some(gate.clone()), drops: drops.clone() })));
            let second = scope.spawn(|| try_install(&slot, || Ok(RacingSource { origin: 11, gate: Some(gate.clone()), drops: drops.clone() })));
            let results = [first.join().unwrap(), second.join().unwrap()];
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            let error = results.into_iter().find_map(|result| result.err()).unwrap();
            assert_eq!(error.current_context(), &ErrorKind::AlreadyExists);
        });
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!([10, 11].contains(&slot.get().unwrap().origin));
        assert_eq!(read_state(slot.get().unwrap()), GlobalManualInstant::ZERO);
        drop(slot);
        assert_eq!(drops.load(Ordering::SeqCst), 2);
    }

    // @trace 5
    /// 本次准备错误（preparation error）不因并发成功安装（concurrent installation）改为重复错误（duplicate error）。
    #[test]
    fn test_install_preparation_error_does_not_revoke_other_success() {
        use std::sync::{Arc, mpsc};
        use std::sync::atomic::AtomicUsize;
        let slot = OnceLock::<GlobalManualState<RacingSource>>::new();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (continue_tx, continue_rx) = mpsc::channel();
        let drops = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            let slot_ref = &slot;
            let failed = scope.spawn(move || try_install(slot_ref, || {
                entered_tx.send(()).unwrap();
                continue_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                Err(Error::new(ErrorKind::Dependency).attach("concurrent-prepare-error"))
            }));
            entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            try_install(&slot, || Ok(RacingSource { origin: 88, gate: None, drops: drops.clone() })).unwrap();
            continue_tx.send(()).unwrap();
            let error = failed.join().unwrap().unwrap_err();
            assert_eq!(error.current_context(), &ErrorKind::Dependency);
            assert!(format!("{error:?}").contains("concurrent-prepare-error"));
        });
        assert_eq!(slot.get().unwrap().origin, 88);
        assert_eq!(read_state(slot.get().unwrap()), GlobalManualInstant::ZERO);
    }

    // @trace 6
    /// 刷新入口（refresh entry）每次只采样（sample）与求间隔（elapsed calculation）各一次；
    /// 固定起点（fixed origin）为100，已知10与20纳秒及相等值（equal value）独立给出期望结果。
    #[test]
    fn test_refresh_uses_fixed_origin_and_equal_candidates_succeed() {
        let state = try_build_state(ControlledSource::new(100)).unwrap();
        let zero = try_refresh_state(&state).unwrap();
        assert_eq!(zero, RefreshOutcome {
            sampled: GlobalManualInstant::ZERO,
            published: GlobalManualInstant::ZERO,
            advanced: false,
        });
        for (sample, expected, advanced) in [(110, 10, true), (110, 10, false), (120, 20, true)] {
            state.source.next.set(sample);
            let outcome = try_refresh_state(&state).unwrap();
            let point = GlobalManualInstant::try_from_elapsed(Duration::from_nanos(expected)).unwrap();
            assert_eq!(outcome, RefreshOutcome { sampled: point, published: point, advanced });
            assert_eq!(read_state(&state), point);
        }
        assert_eq!(state.origin, 100);
        assert_eq!(state.source.samples.get(), 5);
        assert_eq!(state.source.elapsed_calls.get(), 4);
    }

    // @trace 7
    /// 采样与间隔错误（sampling/elapsed errors）保留分类及附件（attachments）；
    /// 已经由生产刷新（production refresh）发布的11纳秒保持，失败后还能从同一起点恢复（recover）。
    #[test]
    fn test_refresh_propagates_original_errors_without_writing() {
        let state = try_build_state(ControlledSource::new(100)).unwrap();
        state.source.next.set(111);
        assert!(try_refresh_state(&state).unwrap().advanced);
        let retained = read_state(&state);
        *state.source.sample_error.borrow_mut() = Some(Error::new(ErrorKind::Dependency)
            .attach("sampling-marker").attach_opaque(101_u32));
        let error = try_refresh_state(&state).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::Dependency);
        assert_eq!(error.downcast_ref::<u32>(), Some(&101));
        assert!(format!("{error:?}").contains("sampling-marker"));
        assert_eq!(read_state(&state), retained);
        assert_eq!(state.source.samples.get(), 3);
        assert_eq!(state.source.elapsed_calls.get(), 1);

        *state.source.elapsed_error.borrow_mut() = Some(Error::new(ErrorKind::InvalidState)
            .attach("elapsed-marker").attach_opaque(102_u32));
        let error = try_refresh_state(&state).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::InvalidState);
        assert_eq!(error.downcast_ref::<u32>(), Some(&102));
        assert!(format!("{error:?}").contains("elapsed-marker"));
        assert_eq!(read_state(&state), retained);
        assert_eq!(state.source.samples.get(), 4);
        assert_eq!(state.source.elapsed_calls.get(), 2);
        state.source.next.set(121);
        assert_eq!(try_refresh_state(&state).unwrap().published.duration_since_origin(), Duration::from_nanos(21));
        assert_eq!(state.origin, 100);
        assert_eq!(state.source.samples.get(), 5);
        assert_eq!(state.source.elapsed_calls.get(), 3);
    }

    /// 不可复制样本（non-Copy sample）不声明Clone；仅由借用（borrow）读取。
    struct OpaqueSample(Duration);

    /// 借用的来源（borrowed source）保留完整标准时长（standard duration），
    /// 不要求静态生命周期（static lifetime）、Send或Sync。
    struct WideSource<'a> {
        next: &'a Cell<Duration>,
        samples: &'a Cell<usize>,
        elapsed_calls: &'a Cell<usize>,
    }

    impl MonotonicSource for WideSource<'_> {
        type Sample = OpaqueSample;
        /// 累计采样次数（sampling count）并包装借用单元（borrowed cell）的完整时长（full duration），不缩窄。
        fn try_sample(&self) -> Result<OpaqueSample> {
            self.samples.set(self.samples.get() + 1);
            Ok(OpaqueSample(self.next.get()))
        }
        /// 累计间隔调用（elapsed calls），用完整时长受检相减（checked subtraction），逆序返回InvalidInput。
        fn try_elapsed(&self, earlier: &OpaqueSample, later: &OpaqueSample) -> Result<Duration> {
            self.elapsed_calls.set(self.elapsed_calls.get() + 1);
            later.0.checked_sub(earlier.0).ok_or_else(|| Error::new(ErrorKind::InvalidInput))
        }
        /// 固定返回未指定的休眠语义（unspecified suspend behavior），不修改借用计数（borrowed counts）。
        fn capabilities(&self) -> SourceCapabilities {
            SourceCapabilities { suspend: SuspendBehavior::Unspecified }
        }
    }

    // @trace 8
    /// u64纳秒上界（nanosecond upper bound）可发布；多1纳秒与Duration::MAX耗尽（exhaustion）
    /// 不截断（truncate）或写缓存（cache），原最大值保持；来源及样本不增加泛型约束（generic bounds）。
    #[test]
    fn test_refresh_rejects_wide_duration_without_losing_maximum() {
        let next = Cell::new(Duration::ZERO);
        let samples = Cell::new(0);
        let elapsed_calls = Cell::new(0);
        let state = try_build_state(WideSource { next: &next, samples: &samples, elapsed_calls: &elapsed_calls }).unwrap();
        next.set(Duration::new(18_446_744_073, 709_551_615));
        assert_eq!(try_refresh_state(&state).unwrap(), RefreshOutcome {
            sampled: GlobalManualInstant::MAX, published: GlobalManualInstant::MAX, advanced: true,
        });
        for duration in [Duration::new(18_446_744_073, 709_551_616), Duration::MAX] {
            next.set(duration);
            let error = try_refresh_state(&state).unwrap_err();
            assert_eq!(error.current_context(), &ErrorKind::ResourceExhausted);
            assert_eq!(read_state(&state), GlobalManualInstant::MAX);
        }
        assert_eq!(samples.get(), 4);
        assert_eq!(elapsed_calls.get(), 3);
    }

    /// 序列来源（sequential source）只在间隔计算（elapsed calculation）挂起旧样本10；
    /// 样本11的生产发布（production publication）完成后，旧调用才恢复（resume）。
    struct DelayedSource {
        samples: std::sync::atomic::AtomicUsize,
        elapsed_calls: std::sync::atomic::AtomicUsize,
        entered: std::sync::Barrier,
        release: std::sync::Barrier,
        fail_late: bool,
    }

    impl MonotonicSource for DelayedSource {
        type Sample = u64;
        /// 按原子调用序号（atomic call index）只提供0、10、11；额外采样（resampling）触发夹具断言（fixture assertion）。
        fn try_sample(&self) -> Result<u64> {
            use std::sync::atomic::Ordering::SeqCst;
            match self.samples.fetch_add(1, SeqCst) {
                0 => Ok(0),
                1 => Ok(10),
                2 => Ok(11),
                _ => panic!("调用不得重新采样（call must not resample）"),
            }
        }
        /// 只把样本10的间隔计算（elapsed calculation）停在两道屏障（barriers），使11先经生产入口发布。
        /// 恢复后可返回一次指定错误（specified error）；只控制来源响应（source response），不修改缓存（cache）。
        fn try_elapsed(&self, earlier: &u64, later: &u64) -> Result<Duration> {
            self.elapsed_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(*earlier, 0);
            if *later == 10 {
                self.entered.wait();
                self.release.wait();
                if self.fail_late {
                    return Err(Error::new(ErrorKind::Dependency).attach("delayed-elapsed-error"));
                }
            }
            Ok(Duration::from_nanos(*later))
        }
        /// 固定返回未指定的休眠语义（unspecified suspend behavior），不参与交错（interleaving）。
        fn capabilities(&self) -> SourceCapabilities {
            SourceCapabilities { suspend: SuspendBehavior::Unspecified }
        }
    }

    // @trace 9
    /// 两个真实刷新（real refreshes）按10采样／11发布／10完成交错（interleaving）；
    /// 迟到成功（late success）与迟到错误（late error）均不能撤销11，各调用只采一次。
    #[test]
    fn test_refresh_late_ten_never_overwrites_eleven_or_revokes_success() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        for fail_late in [false, true] {
            let state = try_build_state(DelayedSource {
                samples: AtomicUsize::new(0), elapsed_calls: AtomicUsize::new(0),
                entered: std::sync::Barrier::new(2), release: std::sync::Barrier::new(2), fail_late,
            }).unwrap();
            std::thread::scope(|scope| {
                let late = scope.spawn(|| try_refresh_state(&state));
                state.source.entered.wait();
                let first = try_refresh_state(&state).unwrap();
                assert_eq!(first.sampled.duration_since_origin(), Duration::from_nanos(11));
                assert_eq!(first.published, first.sampled);
                assert!(first.advanced);
                assert_eq!(read_state(&state), first.published);
                state.source.release.wait();
                let result = late.join().unwrap();
                if fail_late {
                    let error = result.unwrap_err();
                    assert_eq!(error.current_context(), &ErrorKind::Dependency);
                    assert!(format!("{error:?}").contains("delayed-elapsed-error"));
                } else {
                    let outcome = result.unwrap();
                    assert_eq!(outcome.sampled.duration_since_origin(), Duration::from_nanos(10));
                    assert_eq!(outcome.published, first.published);
                    assert!(!outcome.advanced);
                }
                assert_eq!(read_state(&state), first.published);
            });
            assert_eq!(state.source.samples.load(SeqCst), 3);
            assert_eq!(state.source.elapsed_calls.load(SeqCst), 2);
        }
    }

    // @trace 1
    /// 首样本（first sample）只取一次；首采样失败（first-sampling failure）的原报告与来源释放（source disposal）保留。
    #[test]
    fn test_build_state_samples_once_and_releases_failed_source() {
        let source = ControlledSource::new(123);
        let samples = source.samples.clone();
        let elapsed_calls = source.elapsed_calls.clone();
        let drops = source.drops.clone();
        let state = try_build_state(source).unwrap();
        assert_eq!(samples.get(), 1);
        assert_eq!(elapsed_calls.get(), 0);
        assert_eq!(state.origin, 123);
        assert_eq!(state.cached_nanos.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(drops.get(), 0);
        drop(state);
        assert_eq!(drops.get(), 1);

        let failed = ControlledSource::new(456);
        let failed_samples = failed.samples.clone();
        let failed_drops = failed.drops.clone();
        *failed.sample_error.borrow_mut() = Some(Error::new(ErrorKind::Dependency).attach("first-sample-marker").attach_opaque(73_u32));
        let error = match try_build_state(failed) {
            Ok(_) => panic!("失败来源不得产生状态（failed source must not create state）"),
            Err(error) => error,
        };
        assert_eq!(error.current_context(), &ErrorKind::Dependency);
        assert!(format!("{error:?}").contains("first-sample-marker"));
        assert_eq!(error.downcast_ref::<u32>(), Some(&73));
        assert_eq!(failed_samples.get(), 1);
        assert_eq!(failed_drops.get(), 1);
    }
}
