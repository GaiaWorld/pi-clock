// 配对芯（paired inner）：由auto_refresh.rs尾部include!装配到同一模块。

// # 用途
// 同步启动（synchronous startup），返回实际间隔（effective interval）。
fn start_global_inner(interval: Option<Duration>) -> Result<Duration> {
    crate::global::try_capabilities().map_err(|error| {
        error.attach_opaque(ControlEffects { stop_requested: false, worker_joined: false })
    })?;
    try_start_on(&CONTROL, normalize_interval(interval), spawn_ready)
}

// # 用途
// 查询可成立的状态快照（status snapshot）。
fn status_global_inner() -> AutoRefreshStatus {
    snapshot_on(&CONTROL)
}

// # 用途
// 提交停止请求（stop request），不等待退出（exit）。
fn request_stop_global_inner() -> Result<bool> {
    request_stop_on(&CONTROL, notify)
}

// # 用途
// 停止并等待退出（stop and join），唯一取得完整结果（complete result）。
fn stop_join_global_inner() -> Result<bool> {
    if AUTO_WORKER.with(|marker| marker.get()) {
        return Err(Error::new(pi_result::ErrorKind::InvalidState)
            .attach("工作线程不能连接自身（worker cannot join itself）")
            .attach_opaque(ControlEffects { stop_requested: false, worker_joined: false }));
    }
    stop_join_on(&CONTROL, notify)
}

// # 用途
// 完整时长规范化（duration normalization）。
fn normalize_interval_inner(interval: Option<Duration>) -> Duration {
    match interval {
        Some(value) if (MIN_INTERVAL..=MAX_INTERVAL).contains(&value) => value,
        _ => DEFAULT_INTERVAL,
    }
}

// # 用途
// 同一生产唯一启动协议（single-start protocol）。
fn start_on_inner<'a, F>(slot: &'a ControlSlot, interval: Duration, build: F) -> Result<Duration>
where F: FnOnce(&mut StartupGuard<'a>) -> Result<()> {
    use std::sync::atomic::Ordering::SeqCst;
    let no_effect = ControlEffects { stop_requested: false, worker_joined: false };
    if slot.load_full().is_some() {
        return Err(Error::new(pi_result::ErrorKind::AlreadyExists).attach_opaque(no_effect));
    }
    let run = create_run(interval);
    let empty: Option<Arc<RunControl>> = None;
    if slot.compare_and_swap(&empty, Some(run.clone())).is_some() {
        return Err(Error::new(pi_result::ErrorKind::AlreadyExists).attach_opaque(no_effect));
    }
    let mut attempt = StartupGuard { slot, run, thread: None, staged: None, armed: true };
    let prepared = build(&mut attempt);
    match prepared {
        Ok(()) => {
            let thread = attempt.thread.take().ok_or_else(|| {
                Error::new(pi_result::ErrorKind::Corrupted)
                    .attach("就绪启动缺少线程句柄（ready startup missing thread handle）").attach_opaque(no_effect)
            })?;
            attempt.staged = Some(Arc::new(ResourceRecord::Owned(AtomicTake::new(OwnedRun {
                thread, control_error: None,
            }))));
            attempt.run.resources.store(attempt.staged.clone());
            let committed = attempt.run.state.try_update(SeqCst, SeqCst, |old| {
                (old & PHASE_MASK == ControlPhase::Starting as u8)
                    .then_some((old & !PHASE_MASK) | ControlPhase::Owned as u8)
            });
            if committed.is_err() {
                return Err(Error::new(pi_result::ErrorKind::Corrupted)
                    .attach("启动资格已改变（startup qualification changed）").attach_opaque(no_effect));
            }
            attempt.armed = false;
            Ok(interval)
        }
        Err(error) => {
            if attempt.thread.is_none() && attempt.staged.is_none() {
                let expected = Some(attempt.run.clone());
                let _ = slot.compare_and_swap(&expected, None);
                attempt.armed = false;
                return Err(error.attach_opaque(no_effect));
            }
            let mut collection = CollectionGuard {
                slot, run: attempt.run.clone(), original: None, origin: ControlPhase::Starting,
                thread: None, outcome: None, control_error: Some(error), notify_needed: false,
                join_started: false, worker_joined: false, armed: false,
            };
            let claimed = attempt.run.state.try_update(SeqCst, SeqCst, |old| {
                (old & PHASE_MASK == ControlPhase::Starting as u8)
                    .then_some((old & !PHASE_MASK) | ControlPhase::Collecting as u8 | STOP)
            });
            let previous = claimed.map_err(|_| Error::new(pi_result::ErrorKind::Corrupted)
                .attach("启动清理资格已改变（startup cleanup qualification changed）").attach_opaque(no_effect))?;
            collection.armed = true;
            collection.notify_needed = previous & STOP == 0;
            collection.thread = attempt.thread.take();
            if collection.thread.is_none() {
                collection.original = attempt.staged.clone();
                if let Some(ResourceRecord::Owned(owned)) = collection.original.as_deref() {
                    if let Some(owned) = owned.take() {
                        collection.thread = Some(owned.thread);
                        collection.control_error = merge_errors(owned.control_error, collection.control_error.take()).err();
                    }
                }
            }
            attempt.armed = false;
            collect_owned(collection, notify).map(|_| interval)
        }
    }
}

// # 用途
// 一致状态投影（consistent state projection）。
fn snapshot_on_inner(slot: &ControlSlot) -> AutoRefreshStatus {
    loop {
        let Some(run) = slot.load_full() else {
            return AutoRefreshStatus { phase: AutoRefreshPhase::Stopped, interval: None };
        };
        let state = run.state.load(std::sync::atomic::Ordering::SeqCst);
        if !slot.load_full().as_ref().is_some_and(|current| Arc::ptr_eq(current, &run)) { continue; }
        let phase = match state & PHASE_MASK {
            value if value == ControlPhase::Starting as u8 => AutoRefreshPhase::Starting,
            value if value == ControlPhase::Owned as u8 => {
                if state & BODY_DONE != 0 { AutoRefreshPhase::Finished }
                else if state & STOP != 0 { AutoRefreshPhase::StopRequested }
                else { AutoRefreshPhase::Running }
            }
            value if value == ControlPhase::Collecting as u8 => AutoRefreshPhase::Joining,
            value if value == ControlPhase::Completed as u8 => AutoRefreshPhase::Finished,
            _ => AutoRefreshPhase::ExitUnconfirmed,
        };
        return AutoRefreshStatus { phase, interval: Some(run.interval) };
    }
}

// # 用途
// 停止原子发布（atomic stop publication）及通知（notification）。
fn request_stop_on_inner<N>(slot: &ControlSlot, notify: N) -> Result<bool>
where N: FnOnce(&Poller) -> Result<()> {
    use std::sync::atomic::Ordering::SeqCst;
    let no_effect = ControlEffects { stop_requested: false, worker_joined: false };
    let Some(run) = slot.load_full() else { return Ok(false); };
    let mut state = run.state.load(SeqCst);
    loop {
        match state & PHASE_MASK {
            value if value == ControlPhase::Starting as u8 => {
                return Err(Error::new(pi_result::ErrorKind::Busy).attach_opaque(no_effect));
            }
            value if value == ControlPhase::Collecting as u8 || value == ControlPhase::Completed as u8 => return Ok(false),
            value if value == ControlPhase::Owned as u8 => {
                if state & STOP != 0 { return Ok(false); }
                match run.state.compare_exchange(state, state | STOP, SeqCst, SeqCst) {
                    Ok(_) => break,
                    Err(observed) => state = observed,
                }
            }
            _ => return Err(Error::new(pi_result::ErrorKind::Corrupted).attach_opaque(no_effect)),
        }
    }
    let effects = ControlEffects { stop_requested: true, worker_joined: false };
    let poller = run.poller.load_full().ok_or_else(|| {
        Error::new(pi_result::ErrorKind::Corrupted)
            .attach("停止请求缺少等待资源（stop request missing poller）").attach_opaque(effects)
    })?;
    notify(&poller).map(|()| true).map_err(|error| error.attach_opaque(effects))
}

// # 用途
// 同一生产唯一回收协议（single-collector protocol）。
fn stop_join_on_inner<N>(slot: &ControlSlot, notify: N) -> Result<bool>
where N: FnOnce(&Poller) -> Result<()> {
    use std::sync::atomic::Ordering::SeqCst;
    let no_effect = ControlEffects { stop_requested: false, worker_joined: false };
    if AUTO_WORKER.with(|marker| marker.get()) {
        return Err(Error::new(pi_result::ErrorKind::InvalidState).attach_opaque(no_effect));
    }
    let Some(run) = slot.load_full() else { return Ok(false); };
    let mut state = run.state.load(SeqCst);
    loop {
        let origin = match state & PHASE_MASK {
            value if value == ControlPhase::Starting as u8 || value == ControlPhase::Collecting as u8 => {
                return Err(Error::new(pi_result::ErrorKind::Busy).attach_opaque(no_effect));
            }
            value if value == ControlPhase::Owned as u8 => ControlPhase::Owned,
            value if value == ControlPhase::Completed as u8 => ControlPhase::Completed,
            value if value == ControlPhase::ExitUnconfirmed as u8 => {
                let error = run.resources.load_full().and_then(|record| {
                    if let ResourceRecord::Unconfirmed(error) = &*record { error.take() } else { None }
                }).unwrap_or_else(|| Error::new(pi_result::ErrorKind::Corrupted)
                    .attach("退出未确认，诊断已领取或缺失（exit unconfirmed; diagnostic taken or missing）"));
                return Err(error.change_context(pi_result::ErrorKind::Corrupted).attach_opaque(no_effect));
            }
            _ => return Err(Error::new(pi_result::ErrorKind::Corrupted).attach_opaque(no_effect)),
        };
        let mut guard = CollectionGuard {
            slot, run: run.clone(), original: None, origin, thread: None, outcome: None,
            control_error: None, notify_needed: false, join_started: false,
            worker_joined: false, armed: false,
        };
        let collecting = (state & !PHASE_MASK) | ControlPhase::Collecting as u8 | STOP;
        match run.state.compare_exchange(state, collecting, SeqCst, SeqCst) {
            Err(observed) => { state = observed; continue; }
            Ok(previous) => {
                guard.armed = true;
                guard.notify_needed = origin == ControlPhase::Owned && previous & STOP == 0;
            }
        }
        // 资格（qualification）到手之后才加载当前资源；不能跨Owned/Collecting/Owned复用预读记录（preloaded record）。
        guard.original = run.resources.load_full();
        let complete = match (origin, guard.original.as_deref()) {
            (ControlPhase::Owned, Some(ResourceRecord::Owned(owned))) => {
                if let Some(owned) = owned.take() {
                    guard.thread = Some(owned.thread);
                    guard.control_error = owned.control_error;
                    true
                } else { false }
            }
            (ControlPhase::Completed, Some(ResourceRecord::Completed(outcome))) => {
                if let Some(outcome) = outcome.take() {
                    guard.outcome = Some(outcome);
                    guard.worker_joined = true;
                    true
                } else { false }
            }
            _ => false,
        };
        if !complete {
            return Err(Error::new(pi_result::ErrorKind::Corrupted)
                .attach("唯一回收缺少完整资源（single collector missing complete resource）")
                .attach_opaque(ControlEffects { stop_requested: true, worker_joined: false }));
        }
        return collect_owned(guard, notify);
    }
}

// # 用途
// 构造空运行记录（empty run record）。
fn create_run_inner(interval: Duration) -> Arc<RunControl> {
    Arc::new(RunControl {
        interval,
        state: AtomicU8::new(ControlPhase::Starting as u8),
        poller: ArcSwapOption::empty(),
        resources: ArcSwapOption::empty(),
    })
}

// # 用途
// 原生资源准备与就绪握手（native preparation/readiness handshake）。
fn spawn_ready_inner(attempt: &mut StartupGuard<'_>) -> Result<()> {
    use pi_result::InteropResultExt;
    let poller = Arc::new(Poller::new().into_classified_error()?);
    attempt.run.poller.store(Some(poller));
    let (ready, receiver) = oneshot::channel();
    let run = attempt.run.clone();
    attempt.thread = Some(std::thread::Builder::new()
        .name("pi-clock-auto-refresh".to_owned())
        .spawn(move || worker(run, ready)).into_classified_error()?);
    receiver.recv().map_err(|error| {
        Error::new(pi_result::ErrorKind::Cancelled)
            .attach("工作线程未确认就绪（worker readiness disconnected）").attach_opaque(error)
    })
}

// # 用途
// 单一工作线程（worker thread），独占内核等待（kernel waiting）。
fn worker_inner(run: Arc<RunControl>, ready: oneshot::Sender<()>) -> Result<()> {
    AUTO_WORKER.with(|marker| marker.set(true));
    let _exit = WorkerExitGuard { run: &run };
    let poller = run.poller.load_full().ok_or_else(|| {
        Error::new(pi_result::ErrorKind::Corrupted).attach("工作线程缺少等待资源（missing worker poller）")
    })?;
    let mut events = Events::new();
    ready.send(()).map_err(|_| {
        Error::new(pi_result::ErrorKind::Cancelled).attach("启动就绪接收方已离开（startup receiver disconnected）")
    })?;
    run_loop(&run.state, run.interval,
        |deadline| wait_until(&poller, &mut events, deadline),
        || crate::global::try_refresh().map(|_| ()))
}

// # 用途
// 固定延迟循环（fixed-delay loop）。
fn run_loop_inner<W, R>(state: &AtomicU8, interval: Duration, wait: W, refresh: R) -> Result<()>
where W: FnMut(Instant) -> Result<()>, R: FnMut() -> Result<()> {
    use std::sync::atomic::Ordering::SeqCst;
    let mut wait = wait;
    let mut refresh = refresh;
    loop {
        if state.load(SeqCst) & STOP != 0 { return Ok(()); }
        let deadline = Instant::now().checked_add(interval).ok_or_else(|| {
            Error::new(pi_result::ErrorKind::ResourceExhausted)
                .attach("等待期限超界（wait deadline overflow）")
        })?;
        loop {
            if state.load(SeqCst) & STOP != 0 { return Ok(()); }
            wait(deadline)?;
            if state.load(SeqCst) & STOP != 0 { return Ok(()); }
            if Instant::now() >= deadline { break; }
        }
        if state.load(SeqCst) & STOP != 0 { return Ok(()); }
        refresh()?;
    }
}

// # 用途
// 单一内核期限等待（single kernel deadline wait）。
fn wait_until_inner(poller: &Poller, events: &mut Events, deadline: Instant) -> Result<()> {
    use pi_result::InteropResultExt;
    loop {
        events.clear();
        match poller.wait_deadline(events, deadline) {
            Ok(_) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err::<(), _>(error).into_classified_error(),
        }
    }
}

// # 用途
// 最佳努力通知（best-effort notification）。
fn notify_inner(poller: &Poller) -> Result<()> {
    use pi_result::InteropResultExt;
    poller.notify().into_classified_error()
}

// # 用途
// 完整报告合并（complete report combination）。
fn merge_errors_inner(primary: Option<Error>, secondary: Option<Error>) -> Result<()> {
    match (primary, secondary) {
        (None, None) => Ok(()),
        (Some(error), None) | (None, Some(error)) => Err(error),
        (Some(primary), Some(secondary)) => {
            let kind = *primary.current_context();
            let mut errors = primary.expand();
            errors.push(secondary);
            Err(errors.change_context(kind))
        }
    }
}

// # 用途
// 已确认退出结果转换（confirmed-exit conversion）。
fn finish_join_inner(raw: std::thread::Result<Result<()>>) -> Result<()> {
    match raw {
        Ok(outcome) => outcome,
        Err(payload) => {
            let message = if let Some(message) = payload.downcast_ref::<&str>() {
                (*message).to_owned()
            } else if let Some(message) = payload.downcast_ref::<String>() {
                message.clone()
            } else {
                "非字符串恐慌负载（non-string panic payload）".to_owned()
            };
            // 先释放恐慌负载（panic payload），再构造返回报告（return report），
            // 避免析构展开（drop unwind）遗留已初始化的返回值（return value）。
            drop(payload);
            Err(Error::new(pi_result::ErrorKind::Internal)
                .attach("工作线程恐慌，连接已确认退出（worker panic; joined）")
                .attach(message))
        }
    }
}

// # 用途
// 共享启动失败及正常退出回收（shared exit collection）。
fn collect_owned_inner<N>(guard: CollectionGuard<'_>, notify: N) -> Result<bool>
where N: FnOnce(&Poller) -> Result<()> {
    let mut guard = guard;
    if guard.notify_needed {
        let notified = match guard.run.poller.load_full() {
            Some(poller) => notify(&poller),
            None => Err(Error::new(pi_result::ErrorKind::Corrupted)
                .attach("停止通知缺少等待资源（stop notification missing poller）")),
        };
        guard.control_error = merge_errors(guard.control_error.take(), notified.err()).err();
        guard.notify_needed = false;
    }
    if let Some(thread) = guard.thread.take() {
        guard.join_started = true;
        let raw = thread.join();
        guard.worker_joined = true;
        guard.outcome = Some(finish_join(raw));
    }
    if !guard.worker_joined || guard.outcome.is_none() {
        return Err(Error::new(pi_result::ErrorKind::Corrupted)
            .attach("回收缺少已确认退出结果（missing confirmed exit outcome）")
            .attach_opaque(ControlEffects { stop_requested: true, worker_joined: guard.worker_joined }));
    }
    let outcome = guard.outcome.take().unwrap_or(Ok(()));
    guard.outcome = Some(merge_errors(outcome.err(), guard.control_error.take()));
    // 返回对象（return object）先取得完整报告；之后仅本Arc比较交换（identity CAS）和不可恐慌的资格解除。
    let delivered = guard.outcome.take().unwrap_or(Ok(())).map(|()| true).map_err(|error| {
        error.attach_opaque(ControlEffects { stop_requested: true, worker_joined: true })
    });
    let expected = Some(guard.run.clone());
    let _previous = guard.slot.compare_and_swap(&expected, None);
    guard.armed = false;
    delivered
}

// # 用途
// 启动展开恢复（startup unwind recovery）。
fn drop_startup_inner(guard: &mut StartupGuard<'_>) {
    use std::sync::atomic::Ordering::SeqCst;
    if !guard.armed { return; }
    if !guard.slot.load_full().as_ref().is_some_and(|run| Arc::ptr_eq(run, &guard.run))
        || guard.run.state.load(SeqCst) & PHASE_MASK != ControlPhase::Starting as u8 {
        return;
    }
    if guard.thread.is_none() && guard.staged.is_none() {
        let expected = Some(guard.run.clone());
        let _ = guard.slot.compare_and_swap(&expected, None);
        guard.armed = false;
        return;
    }
    let claimed = guard.run.state.try_update(SeqCst, SeqCst, |old| {
        (old & PHASE_MASK == ControlPhase::Starting as u8)
            .then_some((old & !PHASE_MASK) | ControlPhase::Collecting as u8 | STOP)
    });
    let Ok(previous) = claimed else { return; };
    let mut control_error = None;
    if guard.thread.is_none() {
        if let Some(ResourceRecord::Owned(owned)) = guard.staged.as_deref() {
            if let Some(owned) = owned.take() {
                guard.thread = Some(owned.thread);
                control_error = owned.control_error;
            }
        }
    }
    if previous & STOP == 0 {
        let notified = match guard.run.poller.load_full() {
            Some(poller) => notify(&poller),
            None => Err(Error::new(pi_result::ErrorKind::Corrupted)
                .attach("启动展开缺少等待资源（startup unwind missing poller）")),
        };
        control_error = merge_errors(control_error, notified.err()).err();
    }
    let phase = if let Some(thread) = guard.thread.take() {
        guard.run.resources.store(Some(Arc::new(ResourceRecord::Owned(AtomicTake::new(OwnedRun {
            thread, control_error,
        })))));
        ControlPhase::Owned
    } else {
        let error = Error::new(pi_result::ErrorKind::Corrupted)
            .attach("启动展开无法恢复句柄，退出未确认（startup handle unavailable; exit unconfirmed）");
        let error = match merge_errors(Some(error), control_error) {
            Err(error) => error,
            Ok(()) => return,
        };
        guard.run.resources.store(Some(Arc::new(ResourceRecord::Unconfirmed(AtomicTake::new(error)))));
        ControlPhase::ExitUnconfirmed
    };
    // 通知错误（notification error）并入完整资源后才重新公开Owned，避免另一回收者（collector）抢先领取。
    let _ = guard.run.state.try_update(SeqCst, SeqCst, |old| {
        (old & PHASE_MASK == ControlPhase::Collecting as u8)
            .then_some((old & !PHASE_MASK) | phase as u8 | STOP)
    });
    guard.armed = false;
}

// # 用途
// 回收展开恢复（collection unwind recovery）。
fn drop_collection_inner(guard: &mut CollectionGuard<'_>) {
    use std::sync::atomic::Ordering::SeqCst;
    if !guard.armed { return; }
    let current = guard.slot.load_full();
    if !current.as_ref().is_some_and(|run| Arc::ptr_eq(run, &guard.run))
        || guard.run.state.load(SeqCst) & PHASE_MASK != ControlPhase::Collecting as u8 {
        return;
    }

    // 本地所有权（local ownership）优先；尚未领取时才在排他阶段（exclusive phase）加载当前记录。
    let phase = if let Some(thread) = guard.thread.take() {
        guard.run.resources.store(Some(Arc::new(ResourceRecord::Owned(AtomicTake::new(OwnedRun {
            thread, control_error: guard.control_error.take(),
        })))));
        ControlPhase::Owned
    } else if guard.outcome.is_some() || guard.worker_joined {
        let outcome = guard.outcome.take().unwrap_or_else(|| {
            Err(Error::new(pi_result::ErrorKind::Internal)
                .attach("连接已确认，结果转换展开（joined; result conversion unwound）"))
        });
        let outcome = merge_errors(outcome.err(), guard.control_error.take());
        guard.run.resources.store(Some(Arc::new(ResourceRecord::Completed(AtomicTake::new(outcome)))));
        ControlPhase::Completed
    } else {
        if !guard.join_started && guard.original.is_none() {
            guard.original = guard.run.resources.load_full();
        }
        let retained = if !guard.join_started {
            match guard.original.as_deref() {
                Some(ResourceRecord::Owned(value))
                    if guard.origin == ControlPhase::Owned && !value.is_taken() => Some(ControlPhase::Owned),
                Some(ResourceRecord::Completed(value))
                    if guard.origin == ControlPhase::Completed && !value.is_taken() => Some(ControlPhase::Completed),
                _ => None,
            }
        } else { None };
        if let Some(phase) = retained {
            // 未领取报告（unclaimed report）仍完整，不重新取走或用替代错误（replacement error）覆盖。
            phase
        } else {
            let diagnostic = Error::new(pi_result::ErrorKind::Corrupted).attach(
                if guard.join_started {
                    "连接调用展开，退出未确认（join invocation unwound; exit unconfirmed）"
                } else {
                    "回收资源缺失，退出未确认（collection resource missing; exit unconfirmed）"
                });
            let diagnostic = match merge_errors(Some(diagnostic), guard.control_error.take()) {
                Err(error) => error,
                Ok(()) => return,
            };
            guard.run.resources.store(Some(Arc::new(ResourceRecord::Unconfirmed(AtomicTake::new(diagnostic)))));
            ControlPhase::ExitUnconfirmed
        }
    };
    // 完整资源先发布（publish complete resource first），CAS仅改阶段且保留并发完成位（concurrent completion bit）。
    let _ = guard.run.state.try_update(SeqCst, SeqCst, |old| {
        (old & PHASE_MASK == ControlPhase::Collecting as u8)
            .then_some((old & !PHASE_MASK) | phase as u8 | STOP)
    });
    guard.armed = false;
}

// # 用途
// 工作主体完成发布（worker-body completion publication）。
fn drop_worker_inner(guard: &mut WorkerExitGuard<'_>) {
    guard.run.state.fetch_or(BODY_DONE, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    //! 同一私有生产协议（same private production protocol）的有界验证（bounded verification）。
    //! 局部槽（local slots）及既有闭包（closures）只提供输入，不代补控制或回收（collection）。
    use super::*;
    use pi_result::ErrorKind;
    use std::sync::atomic::Ordering::SeqCst;

    /// 完整时长选择（duration selection）保留两端点及纳秒精度（nanosecond precision），超界统一默认。
    // @trace 1
    #[test]
    fn test_interval_domain() {
        for input in [None, Some(Duration::ZERO), Some(Duration::from_nanos(99_999)),
            Some(Duration::new(1, 1)), Some(Duration::MAX)] {
            assert_eq!(normalize_interval(input), Duration::from_millis(1));
        }
        for value in [Duration::from_micros(100), Duration::from_nanos(100_001),
            Duration::from_nanos(999_999_999), Duration::from_secs(1)] {
            assert_eq!(normalize_interval(Some(value)), value);
        }
    }

    /// 合并报告（report merge）保留两分支附件（branch attachments）及首要分类（primary kind），单错原样移动。
    // @trace 2
    #[test]
    fn test_merge_errors_preserves_branches() {
        assert!(merge_errors(None, None).is_ok());
        for first in [true, false] {
            let error = Error::new(ErrorKind::Dependency).attach("single-report").attach_opaque(17_u32);
            let result = if first { merge_errors(Some(error), None) } else { merge_errors(None, Some(error)) };
            let error = result.unwrap_err();
            assert_eq!(error.current_context(), &ErrorKind::Dependency);
            assert_eq!(error.downcast_ref::<u32>(), Some(&17));
        }
        let error = merge_errors(
            Some(Error::new(ErrorKind::Dependency).attach("worker-report").attach_opaque(23_u32)),
            Some(Error::new(ErrorKind::Io).attach("notify-report").attach_opaque(29_u64)),
        ).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::Dependency);
        assert_eq!(error.downcast_ref::<u32>(), Some(&23));
        assert_eq!(error.downcast_ref::<u64>(), Some(&29));
        assert!(format!("{error:?}").contains("worker-report"));
        assert!(format!("{error:?}").contains("notify-report"));
        assert_eq!(error.frames().filter(|frame| frame.downcast_ref::<ErrorKind>() == Some(&ErrorKind::Io)).count(), 1);
    }

    /// 已确认连接结果（confirmed join result）透传业务错误（business error），三类恐慌负载（panic payload）均转换为Internal。
    // @trace 3
    #[test]
    fn test_finish_join_preserves_result_and_converts_panic() {
        assert!(finish_join(Ok(Ok(()))).is_ok());
        let error = finish_join(Ok(Err(Error::new(ErrorKind::Unavailable).attach_opaque(31_u32)))).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::Unavailable);
        assert_eq!(error.downcast_ref::<u32>(), Some(&31));
        for payload in [Box::new("literal-panic") as Box<dyn std::any::Any + Send>,
            Box::new(String::from("owned-panic")), Box::new(37_u32)] {
            let error = finish_join(Err(payload)).unwrap_err();
            assert_eq!(error.current_context(), &ErrorKind::Internal);
            let message = format!("{error:?}");
            assert!(message.contains("literal-panic") || message.contains("owned-panic") || message.contains("non-string"));
        }
    }

    /// 新运行（new run）保留独立身份（distinct identity），尚无内核资源（kernel resources）及停止/完成事实。
    // @trace 4
    #[test]
    fn test_new_run_is_unprepared_and_unique() {
        let first = create_run(Duration::from_micros(101));
        let second = create_run(Duration::from_micros(101));
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(first.interval, Duration::from_micros(101));
        assert_eq!(first.state.load(SeqCst), ControlPhase::Starting as u8);
        assert!(first.poller.load_full().is_none());
        assert!(first.resources.load_full().is_none());
    }

    /// 真实等待（real wait）允许通知空返回（notification empty return）；再次等待仍使用原期限（original deadline）。
    // @trace 5
    #[test]
    fn test_wait_until_uses_real_poller_and_original_deadline() {
        let poller = Poller::new().unwrap();
        let mut events = Events::new();
        let deadline = Instant::now() + Duration::from_millis(30);
        poller.notify().unwrap();
        wait_until(&poller, &mut events, deadline).unwrap();
        assert!(events.is_empty());
        wait_until(&poller, &mut events, deadline).unwrap();
        assert!(Instant::now() >= deadline);
        assert!(events.is_empty());
    }

    /// 生产通知（production notification）唤醒下一次真实等待（real wait），不依赖用户文件描述符（user FD）。
    // @trace 6
    #[test]
    fn test_notify_wakes_next_wait() {
        let poller = Poller::new().unwrap();
        let mut events = Events::new();
        let deadline = Instant::now() + Duration::from_secs(1);
        notify(&poller).unwrap();
        wait_until(&poller, &mut events, deadline).unwrap();
        assert!(Instant::now() < deadline);
        assert!(events.is_empty());
    }

    /// 固定延迟（fixed delay）保留早醒期限（early-wake deadline），晚醒（late wake）只刷新一次，再从成功后完整等待。
    // @trace 7
    #[test]
    fn test_loop_fixed_delay_and_failures() {
        use std::cell::Cell;
        let state = AtomicU8::new(0);
        let first_deadline = Cell::new(None);
        let refresh_finished = Cell::new(None);
        let calls = Cell::new(0);
        let refreshed = Cell::new(0);
        let interval = Duration::from_millis(20);
        run_loop(&state, interval, |deadline| {
            let call = calls.get();
            calls.set(call + 1);
            match call {
                0 => { first_deadline.set(Some(deadline)); }
                1 => {
                    assert_eq!(Some(deadline), first_deadline.get());
                    std::thread::sleep(deadline.saturating_duration_since(Instant::now()) + interval);
                }
                2 => {
                    assert_eq!(refreshed.get(), 1);
                    assert!(deadline >= refresh_finished.get().unwrap() + interval);
                    state.fetch_or(STOP, SeqCst);
                }
                _ => panic!("不得补跑（no catch-up）"),
            }
            Ok(())
        }, || {
            assert!(Instant::now() >= first_deadline.get().unwrap());
            refreshed.set(refreshed.get() + 1);
            refresh_finished.set(Some(Instant::now()));
            Ok(())
        }).unwrap();
        assert_eq!(calls.get(), 3);
        assert_eq!(refreshed.get(), 1);
        for sampling in [false, true] {
            let state = AtomicU8::new(0);
            let error = run_loop(&state, MIN_INTERVAL, |deadline| {
                if !sampling { return Err(Error::new(ErrorKind::Cancelled).attach_opaque(41_u32)); }
                std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
                Ok(())
            }, || Err(Error::new(ErrorKind::Dependency).attach_opaque(43_u64))).unwrap_err();
            assert_eq!(error.current_context(), &if sampling { ErrorKind::Dependency } else { ErrorKind::Cancelled });
            if sampling { assert_eq!(error.downcast_ref::<u64>(), Some(&43)); }
            else { assert_eq!(error.downcast_ref::<u32>(), Some(&41)); }
        }
        let stopped = AtomicU8::new(STOP);
        run_loop(&stopped, DEFAULT_INTERVAL, |_| panic!("停止后不得等待（no wait after stop）"),
            || panic!("停止后不得刷新（no refresh after stop）")).unwrap();
        let error = run_loop(&AtomicU8::new(0), Duration::MAX,
            |_| panic!("期限溢出不得等待（no wait after overflow）"), || panic!("no refresh")).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::ResourceExhausted);
    }

    /// 主体退出发布（body-exit publication）在正常与展开（unwind）路径保持阶段及STOP，不等同连接（join）。
    // @trace 8
    #[test]
    fn test_worker_exit_preserves_phase_and_stop() {
        let run = create_run(DEFAULT_INTERVAL);
        run.state.store(ControlPhase::Collecting as u8 | STOP, SeqCst);
        let mut guard = std::mem::ManuallyDrop::new(WorkerExitGuard { run: &run });
        drop_worker_inner(&mut guard);
        assert_eq!(run.state.load(SeqCst), ControlPhase::Collecting as u8 | STOP | BODY_DONE);
        let run = create_run(DEFAULT_INTERVAL);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = WorkerExitGuard { run: &run };
            panic!("worker-unwind");
        }));
        assert!(result.is_err());
        assert_eq!(run.state.load(SeqCst), ControlPhase::Starting as u8 | BODY_DONE);
    }

    /// 工作装配（worker assembly）使用真实Poller和就绪通道（readiness channel）；断连（disconnect）报告Cancelled并发布完成。
    // @trace 9
    #[test]
    fn test_worker_ready_stop_and_disconnect() {
        for disconnected in [false, true] {
            let run = create_run(DEFAULT_INTERVAL);
            run.poller.store(Some(Arc::new(Poller::new().unwrap())));
            run.state.fetch_or(STOP, SeqCst);
            let (sender, receiver) = oneshot::channel();
            if disconnected { drop(receiver); }
            else {
                let worker_run = run.clone();
                let thread = std::thread::spawn(move || worker(worker_run, sender));
                receiver.recv_timeout(Duration::from_secs(5)).unwrap();
                thread.join().unwrap().unwrap();
                assert_ne!(run.state.load(SeqCst) & BODY_DONE, 0);
                continue;
            }
            let worker_run = run.clone();
            let error = std::thread::spawn(move || worker(worker_run, sender)).join().unwrap().unwrap_err();
            assert_eq!(error.current_context(), &ErrorKind::Cancelled);
            assert_ne!(run.state.load(SeqCst) & BODY_DONE, 0);
        }
        let run = create_run(Duration::from_millis(30));
        run.poller.store(Some(Arc::new(Poller::new().unwrap())));
        let (sender, receiver) = oneshot::channel();
        let worker_run = run.clone();
        let started = Instant::now();
        let thread = std::thread::spawn(move || worker(worker_run, sender));
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let error = thread.join().unwrap().unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::InvalidState);
        assert!(started.elapsed() >= Duration::from_millis(30));
        assert_ne!(run.state.load(SeqCst) & BODY_DONE, 0);
    }

    /// 仅构造合法恢复检查点（recovery checkpoint）；不代补资源发布（resource publication）或阶段恢复（phase recovery）。
    fn collection_guard<'a>(slot: &'a ControlSlot, run: Arc<RunControl>, origin: ControlPhase) -> CollectionGuard<'a> {
        CollectionGuard { slot, run, original: None, origin, thread: None, outcome: None,
            control_error: None, notify_needed: false, join_started: false, worker_joined: false, armed: true }
    }

    /// 已领取资格但尚未领取结果（claimed qualification/unclaimed outcome）的展开（unwind）必须保留Completed真实报告。
    // @trace 10
    #[test]
    fn test_collection_drop_preserves_unclaimed_completed() {
        let slot = ControlSlot::empty();
        let run = create_run(DEFAULT_INTERVAL);
        let record = Arc::new(ResourceRecord::Completed(AtomicTake::new(Err(
            Error::new(ErrorKind::Dependency).attach("original-completed").attach_opaque(47_u32)))));
        run.resources.store(Some(record.clone()));
        run.state.store(ControlPhase::Collecting as u8 | STOP | BODY_DONE, SeqCst);
        slot.store(Some(run.clone()));
        let mut guard = collection_guard(&slot, run.clone(), ControlPhase::Completed);
        drop_collection_inner(&mut guard);
        assert_eq!(run.state.load(SeqCst), ControlPhase::Completed as u8 | STOP | BODY_DONE);
        assert!(Arc::ptr_eq(&run.resources.load_full().unwrap(), &record));
        let ResourceRecord::Completed(outcome) = &*record else { panic!("wrong record") };
        let error = outcome.take().unwrap().unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::Dependency);
        assert_eq!(error.downcast_ref::<u32>(), Some(&47));
    }

    /// 回收保护（collection guard）覆盖句柄已领取/未领取（taken/untaken handle）、启动清理（startup cleanup）、
    /// 已确认结果（confirmed outcome）及未确认检查点（unconfirmed checkpoint）；不模拟join调用自身恐慌的实际发生。
    // @trace 11
    #[test]
    fn test_collection_drop_recovery_variants() {
        for origin in [ControlPhase::Starting, ControlPhase::Owned] {
            let slot = ControlSlot::empty();
            let run = create_run(DEFAULT_INTERVAL);
            run.state.store(ControlPhase::Collecting as u8 | STOP | BODY_DONE, SeqCst);
            slot.store(Some(run.clone()));
            let mut guard = collection_guard(&slot, run.clone(), origin);
            guard.thread = Some(std::thread::spawn(|| Ok(())));
            guard.control_error = Some(Error::new(ErrorKind::Io).attach_opaque(53_u32));
            drop(guard);
            assert_eq!(run.state.load(SeqCst), ControlPhase::Owned as u8 | STOP | BODY_DONE);
            let record = run.resources.load_full().unwrap();
            let ResourceRecord::Owned(owned) = &*record else { panic!("wrong record") };
            let owned = owned.take().unwrap();
            assert_eq!(owned.control_error.unwrap().downcast_ref::<u32>(), Some(&53));
            owned.thread.join().unwrap().unwrap();
        }
        let slot = ControlSlot::empty();
        let run = create_run(DEFAULT_INTERVAL);
        let record = Arc::new(ResourceRecord::Owned(AtomicTake::new(OwnedRun {
            thread: std::thread::spawn(|| Ok(())), control_error: None,
        })));
        run.resources.store(Some(record.clone()));
        run.state.store(ControlPhase::Collecting as u8 | STOP, SeqCst);
        slot.store(Some(run.clone()));
        drop(collection_guard(&slot, run.clone(), ControlPhase::Owned));
        assert!(Arc::ptr_eq(&run.resources.load_full().unwrap(), &record));
        let ResourceRecord::Owned(owned) = &*record else { panic!("wrong record") };
        owned.take().unwrap().thread.join().unwrap().unwrap();

        for has_outcome in [true, false] {
            let slot = ControlSlot::empty();
            let run = create_run(DEFAULT_INTERVAL);
            run.state.store(ControlPhase::Collecting as u8 | STOP | BODY_DONE, SeqCst);
            slot.store(Some(run.clone()));
            let mut guard = collection_guard(&slot, run.clone(), ControlPhase::Owned);
            guard.join_started = true;
            guard.worker_joined = true;
            guard.outcome = if has_outcome { Some(Err(Error::new(ErrorKind::Dependency).attach_opaque(59_u32))) } else { None };
            guard.control_error = Some(Error::new(ErrorKind::Io).attach_opaque(61_u64));
            drop(guard);
            assert_eq!(run.state.load(SeqCst), ControlPhase::Completed as u8 | STOP | BODY_DONE);
            let record = run.resources.load_full().unwrap();
            let ResourceRecord::Completed(outcome) = &*record else { panic!("wrong record") };
            let error = outcome.take().unwrap().unwrap_err();
            assert_eq!(error.current_context(), &if has_outcome { ErrorKind::Dependency } else { ErrorKind::Internal });
            assert_eq!(error.downcast_ref::<u64>(), Some(&61));
            if has_outcome { assert_eq!(error.downcast_ref::<u32>(), Some(&59)); }
        }
        let slot = ControlSlot::empty();
        let run = create_run(DEFAULT_INTERVAL);
        run.state.store(ControlPhase::Collecting as u8 | STOP, SeqCst);
        slot.store(Some(run.clone()));
        let mut guard = collection_guard(&slot, run.clone(), ControlPhase::Owned);
        guard.join_started = true;
        guard.control_error = Some(Error::new(ErrorKind::Io).attach_opaque(67_u64));
        drop(guard);
        assert_eq!(run.state.load(SeqCst), ControlPhase::ExitUnconfirmed as u8 | STOP);
        assert!(Arc::ptr_eq(&slot.load_full().unwrap(), &run));
        let record = run.resources.load_full().unwrap();
        let ResourceRecord::Unconfirmed(error) = &*record else { panic!("wrong record") };
        let error = error.take().unwrap();
        assert_eq!(error.current_context(), &ErrorKind::Corrupted);
        assert_eq!(error.downcast_ref::<u64>(), Some(&67));
    }

    /// 通知失败（notification failure）仍真实连接（join），工作/控制两分支（worker/control branches）及效果（effects）完整交付。
    // @trace 12
    #[test]
    fn test_collect_joins_despite_notify_failure() {
        for worker_fails in [false, true] {
            let slot = ControlSlot::empty();
            let run = create_run(DEFAULT_INTERVAL);
            run.poller.store(Some(Arc::new(Poller::new().unwrap())));
            run.state.store(ControlPhase::Collecting as u8 | STOP, SeqCst);
            slot.store(Some(run.clone()));
            let mut guard = collection_guard(&slot, run.clone(), ControlPhase::Starting);
            let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let worker_done = done.clone();
            guard.thread = Some(std::thread::spawn(move || {
                worker_done.store(true, SeqCst);
                if worker_fails { Err(Error::new(ErrorKind::Dependency).attach_opaque(71_u32)) } else { Ok(()) }
            }));
            guard.notify_needed = true;
            guard.control_error = Some(Error::new(ErrorKind::Cancelled).attach_opaque(73_u16));
            let error = collect_owned(guard, |_| Err(Error::new(ErrorKind::Io).attach_opaque(79_u64))).unwrap_err();
            assert!(done.load(SeqCst));
            assert!(slot.load_full().is_none());
            assert_eq!(error.current_context(), &if worker_fails { ErrorKind::Dependency } else { ErrorKind::Cancelled });
            if worker_fails { assert_eq!(error.downcast_ref::<u32>(), Some(&71)); }
            assert_eq!(error.downcast_ref::<u16>(), Some(&73));
            assert_eq!(error.downcast_ref::<u64>(), Some(&79));
            assert_eq!(error.downcast_ref::<ControlEffects>(), Some(&ControlEffects { stop_requested: true, worker_joined: true }));
        }
        let slot = ControlSlot::empty();
        let run = create_run(DEFAULT_INTERVAL);
        run.state.store(ControlPhase::Collecting as u8 | STOP | BODY_DONE, SeqCst);
        slot.store(Some(run.clone()));
        let mut guard = collection_guard(&slot, run, ControlPhase::Completed);
        guard.worker_joined = true;
        guard.outcome = Some(Ok(()));
        assert!(collect_owned(guard, |_| panic!("Completed不得通知（no notify for Completed）")).unwrap());
        assert!(slot.load_full().is_none());
    }

    /// 启动展开恢复（startup unwind recovery）只释放无线程资格（threadless qualification），有句柄则发布可回收记录（collectible record）。
    // @trace 13
    #[test]
    fn test_startup_drop_restores_owned_without_join() {
        for staged in [false, true] {
            let slot = ControlSlot::empty();
            let run = create_run(DEFAULT_INTERVAL);
            run.poller.store(Some(Arc::new(Poller::new().unwrap())));
            slot.store(Some(run.clone()));
            let thread = std::thread::spawn(|| Ok(()));
            let mut guard = StartupGuard {
                slot: &slot, run: run.clone(), thread: None, staged: None, armed: true,
            };
            if staged {
                let record = Arc::new(ResourceRecord::Owned(AtomicTake::new(OwnedRun { thread, control_error: None })));
                guard.staged = Some(record.clone());
                run.resources.store(Some(record));
            } else { guard.thread = Some(thread); }
            drop_startup_inner(&mut guard);
            assert_eq!(run.state.load(SeqCst) & (PHASE_MASK | STOP), ControlPhase::Owned as u8 | STOP);
            let record = run.resources.load_full().unwrap();
            let ResourceRecord::Owned(owned) = &*record else { panic!("wrong record") };
            owned.take().unwrap().thread.join().unwrap().unwrap();
        }
        let slot = ControlSlot::empty();
        let run = create_run(DEFAULT_INTERVAL);
        slot.store(Some(run.clone()));
        drop(StartupGuard { slot: &slot, run, thread: None, staged: None, armed: true });
        assert!(slot.load_full().is_none());
    }

    /// 状态快照（status snapshot）从同一状态字（state word）投影；主体完成（body done）优先于停止请求（stop request）。
    // @trace 14
    #[test]
    fn test_snapshot_projects_one_state_without_consuming() {
        let slot = ControlSlot::empty();
        assert_eq!(snapshot_on(&slot), AutoRefreshStatus { phase: AutoRefreshPhase::Stopped, interval: None });
        let run = create_run(Duration::from_micros(123));
        slot.store(Some(run.clone()));
        for (state, phase) in [
            (ControlPhase::Starting as u8, AutoRefreshPhase::Starting),
            (ControlPhase::Owned as u8, AutoRefreshPhase::Running),
            (ControlPhase::Owned as u8 | STOP, AutoRefreshPhase::StopRequested),
            (ControlPhase::Owned as u8 | STOP | BODY_DONE, AutoRefreshPhase::Finished),
            (ControlPhase::Collecting as u8 | STOP, AutoRefreshPhase::Joining),
            (ControlPhase::Completed as u8 | STOP | BODY_DONE, AutoRefreshPhase::Finished),
            (ControlPhase::ExitUnconfirmed as u8 | STOP, AutoRefreshPhase::ExitUnconfirmed),
        ] {
            run.state.store(state, SeqCst);
            assert_eq!(snapshot_on(&slot), AutoRefreshStatus { phase, interval: Some(Duration::from_micros(123)) });
            assert_eq!(run.state.load(SeqCst), state);
        }
    }

    /// 停止请求（stop request）先原子提交（atomic commit），失败通知（failed notification）不回滚；重复和不可操作阶段不通知。
    // @trace 15
    #[test]
    fn test_request_stop_is_first_only_and_preserves_effects() {
        let slot = ControlSlot::empty();
        assert!(!request_stop_on(&slot, |_| panic!("empty notify")).unwrap());
        let run = create_run(DEFAULT_INTERVAL);
        slot.store(Some(run.clone()));
        let error = request_stop_on(&slot, |_| panic!("starting notify")).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::Busy);
        assert_eq!(error.downcast_ref::<ControlEffects>(), Some(&ControlEffects { stop_requested: false, worker_joined: false }));
        run.poller.store(Some(Arc::new(Poller::new().unwrap())));
        run.state.store(ControlPhase::Owned as u8 | BODY_DONE, SeqCst);
        let error = request_stop_on(&slot, |_| {
            assert_ne!(run.state.load(SeqCst) & STOP, 0);
            Err(Error::new(ErrorKind::Io).attach_opaque(83_u32))
        }).unwrap_err();
        assert_eq!(error.downcast_ref::<u32>(), Some(&83));
        assert_eq!(error.downcast_ref::<ControlEffects>(), Some(&ControlEffects { stop_requested: true, worker_joined: false }));
        assert_eq!(run.state.load(SeqCst), ControlPhase::Owned as u8 | BODY_DONE | STOP);
        assert!(!request_stop_on(&slot, |_| panic!("duplicate notify")).unwrap());
        for phase in [ControlPhase::Collecting, ControlPhase::Completed] {
            run.state.store(phase as u8 | STOP | BODY_DONE, SeqCst);
            assert!(!request_stop_on(&slot, |_| panic!("phase notify")).unwrap());
        }
        let diagnostic = Arc::new(ResourceRecord::Unconfirmed(AtomicTake::new(Error::new(ErrorKind::Internal).attach_opaque(89_u64))));
        run.resources.store(Some(diagnostic.clone()));
        run.state.store(ControlPhase::ExitUnconfirmed as u8 | STOP, SeqCst);
        assert_eq!(request_stop_on(&slot, |_| panic!("unconfirmed notify")).unwrap_err().current_context(), &ErrorKind::Corrupted);
        let ResourceRecord::Unconfirmed(error) = &*diagnostic else { panic!("wrong record") };
        assert_eq!(error.take().unwrap().downcast_ref::<u64>(), Some(&89));
    }

    /// 唯一回收（single collection）在通知闭包（notify closure）重入时拒绝Busy，自身标记（self marker）优先InvalidState。
    // @trace 16
    #[test]
    fn test_stop_join_claims_once_and_rejects_self_first() {
        let slot = ControlSlot::empty();
        assert!(!stop_join_on(&slot, |_| panic!("empty notify")).unwrap());
        let run = create_run(DEFAULT_INTERVAL);
        run.poller.store(Some(Arc::new(Poller::new().unwrap())));
        run.resources.store(Some(Arc::new(ResourceRecord::Owned(AtomicTake::new(OwnedRun {
            thread: std::thread::spawn(|| Ok(())), control_error: None,
        })))));
        run.state.store(ControlPhase::Owned as u8 | BODY_DONE, SeqCst);
        slot.store(Some(run.clone()));
        assert!(stop_join_on(&slot, |_| {
            assert_eq!(stop_join_on(&slot, |_| panic!("loser notify")).unwrap_err().current_context(), &ErrorKind::Busy);
            AUTO_WORKER.with(|marker| marker.set(true));
            let result = stop_join_on(&slot, |_| panic!("self notify"));
            AUTO_WORKER.with(|marker| marker.set(false));
            assert_eq!(result.unwrap_err().current_context(), &ErrorKind::InvalidState);
            Ok(())
        }).unwrap());
        assert!(slot.load_full().is_none());
        assert!(!stop_join_on(&slot, |_| panic!("duplicate notify")).unwrap());

        let run = create_run(DEFAULT_INTERVAL);
        run.resources.store(Some(Arc::new(ResourceRecord::Completed(AtomicTake::new(Err(
            Error::new(ErrorKind::Dependency).attach_opaque(97_u32)))))));
        run.state.store(ControlPhase::Completed as u8 | STOP | BODY_DONE, SeqCst);
        slot.store(Some(run));
        let error = stop_join_on(&slot, |_| panic!("Completed notify")).unwrap_err();
        assert_eq!(error.downcast_ref::<u32>(), Some(&97));
        assert_eq!(error.downcast_ref::<ControlEffects>(), Some(&ControlEffects { stop_requested: true, worker_joined: true }));
        assert!(slot.load_full().is_none());
    }

    /// 启动协议（startup protocol）覆盖准备失败重试（preparation retry）、已有线程失败回收（spawned failure collection）及重复拒绝。
    // @trace 17
    #[test]
    fn test_start_protocol_retries_and_collects_failures() {
        let slot = ControlSlot::empty();
        let error = try_start_on(&slot, DEFAULT_INTERVAL, |_| {
            Err(Error::new(ErrorKind::Unavailable).attach_opaque(101_u32))
        }).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::Unavailable);
        assert_eq!(error.downcast_ref::<u32>(), Some(&101));
        assert!(slot.load_full().is_none());
        let error = try_start_on(&slot, DEFAULT_INTERVAL, |attempt| {
            attempt.run.poller.store(Some(Arc::new(Poller::new().unwrap())));
            attempt.thread = Some(std::thread::spawn(|| Err(Error::new(ErrorKind::Dependency).attach_opaque(103_u64))));
            Err(Error::new(ErrorKind::Cancelled).attach_opaque(107_u16))
        }).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::Dependency);
        assert_eq!(error.downcast_ref::<u64>(), Some(&103));
        assert_eq!(error.downcast_ref::<u16>(), Some(&107));
        assert_eq!(error.downcast_ref::<ControlEffects>(), Some(&ControlEffects { stop_requested: true, worker_joined: true }));
        assert!(slot.load_full().is_none());
        assert_eq!(try_start_on(&slot, MIN_INTERVAL, |attempt| {
            attempt.run.poller.store(Some(Arc::new(Poller::new().unwrap())));
            attempt.thread = Some(std::thread::spawn(|| Ok(())));
            attempt.run.state.fetch_or(BODY_DONE, SeqCst);
            Ok(())
        }).unwrap(), MIN_INTERVAL);
        assert_eq!(snapshot_on(&slot).phase, AutoRefreshPhase::Finished);
        assert_eq!(try_start_on(&slot, DEFAULT_INTERVAL, |_| panic!("duplicate build")).unwrap_err().current_context(), &ErrorKind::AlreadyExists);
        assert!(stop_join_on(&slot, notify).unwrap());
        assert!(slot.load_full().is_none());
    }

    /// 原生就绪握手（native readiness handshake）返回后已有受保护句柄（guarded handle），后续业务失败（business failure）仍可完整回收。
    // @trace 18
    #[test]
    fn test_spawn_ready_uses_real_native_worker() {
        let slot = ControlSlot::empty();
        assert_eq!(try_start_on(&slot, Duration::from_millis(40), spawn_ready).unwrap(), Duration::from_millis(40));
        let run = slot.load_full().unwrap();
        assert!(run.poller.load_full().is_some());
        assert!(run.resources.load_full().is_some());
        let deadline = Instant::now() + Duration::from_secs(5);
        while run.state.load(SeqCst) & BODY_DONE == 0 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(1));
        }
        let error = stop_join_on(&slot, notify).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::InvalidState);
        assert_eq!(error.downcast_ref::<ControlEffects>(), Some(&ControlEffects { stop_requested: true, worker_joined: true }));
        assert!(slot.load_full().is_none());
    }

    /// 真实展开（real unwind）通过既有构建闭包（build closure）触发；有线程（thread）时由启动保护（startup guard）恢复，随后显式回收（explicit collection）。
    /// 此局部槽（local slot）未安装global；若调度（scheduling）跨过首期限（first deadline），允许透传真实InvalidState，但必须确认退出及清槽（confirmed exit/slot clearance）。
    // @trace 19
    #[test]
    fn test_startup_unwind_recovers_real_worker_and_threadless_attempt() {
        for spawned in [false, true] {
            let slot = ControlSlot::empty();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                try_start_on(&slot, MAX_INTERVAL, |attempt| {
                    if spawned { spawn_ready(attempt)?; }
                    panic!("startup-build-unwind");
                })
            }));
            assert!(result.is_err());
            if spawned {
                let run = slot.load_full().unwrap();
                assert_eq!(run.state.load(SeqCst) & (PHASE_MASK | STOP), ControlPhase::Owned as u8 | STOP);
                assert!(run.resources.load_full().is_some());
                match stop_join_on(&slot, notify) {
                    Ok(joined) => assert!(joined),
                    Err(error) => {
                        assert_eq!(error.current_context(), &ErrorKind::InvalidState);
                        assert_eq!(error.downcast_ref::<ControlEffects>(), Some(&ControlEffects {
                            stop_requested: true, worker_joined: true,
                        }));
                    }
                }
                assert_ne!(run.state.load(SeqCst) & BODY_DONE, 0);
            }
            assert!(slot.load_full().is_none());
        }
    }

    /// 通知闭包展开（notification-closure unwind）真实经过唯一回收（single collection）和保护恢复（guard recovery）。
    /// 恢复后的回收（post-recovery collection）必须使用新记录（new record），保留工作及控制报告（worker/control reports）；不声称暂停了竞争者的CAS前窗口（pre-CAS window）。
    // @trace 20
    #[test]
    fn test_collection_unwind_replaces_taken_record_and_recollects() {
        let slot = ControlSlot::empty();
        let run = create_run(DEFAULT_INTERVAL);
        run.poller.store(Some(Arc::new(Poller::new().unwrap())));
        let worker_run = run.clone();
        let thread = std::thread::spawn(move || {
            let _exit = WorkerExitGuard { run: &worker_run };
            Err(Error::new(ErrorKind::Dependency).attach_opaque(211_u32))
        });
        let before = Arc::new(ResourceRecord::Owned(AtomicTake::new(OwnedRun {
            thread, control_error: Some(Error::new(ErrorKind::Io).attach_opaque(223_u64)),
        })));
        run.resources.store(Some(before.clone()));
        run.state.try_update(SeqCst, SeqCst, |old| {
            Some((old & !PHASE_MASK) | ControlPhase::Owned as u8)
        }).unwrap();
        slot.store(Some(run.clone()));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stop_join_on(&slot, |_| panic!("collection-notify-unwind"))
        }));
        assert!(result.is_err());
        assert_eq!(run.state.load(SeqCst) & (PHASE_MASK | STOP), ControlPhase::Owned as u8 | STOP);
        let restored = run.resources.load_full().unwrap();
        assert!(!Arc::ptr_eq(&before, &restored));
        let ResourceRecord::Owned(previous) = &*before else { panic!("wrong original record") };
        assert!(previous.is_taken());
        let ResourceRecord::Owned(current) = &*restored else { panic!("wrong restored record") };
        assert!(!current.is_taken());
        let error = stop_join_on(&slot, |_| panic!("restored STOP must not notify again")).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::Dependency);
        assert_eq!(error.downcast_ref::<u32>(), Some(&211));
        assert_eq!(error.downcast_ref::<u64>(), Some(&223));
        assert_eq!(error.downcast_ref::<ControlEffects>(), Some(&ControlEffects { stop_requested: true, worker_joined: true }));
        assert!(slot.load_full().is_none());
        assert!(current.is_taken());
        assert_ne!(run.state.load(SeqCst) & BODY_DONE, 0);
    }

    /// 旧运行延后通知（delayed old-run notification）在同一生产协议（production protocol）完成回收及重启（collection/restart）后仍只收到旧Poller。
    /// 局部工作结果（local worker outcome）仅提供Ok，不模拟等待循环（wait loop）；断言检查身份隔离及新运行STOP不变（identity isolation/persistent new-run STOP）。
    // @trace 21
    #[test]
    fn test_delayed_old_notification_isolated_from_restarted_run() {
        let slot = ControlSlot::empty();
        let build = |attempt: &mut StartupGuard<'_>| {
            attempt.run.poller.store(Some(Arc::new(Poller::new().unwrap())));
            let run = attempt.run.clone();
            attempt.thread = Some(std::thread::spawn(move || {
                let _exit = WorkerExitGuard { run: &run };
                Ok(())
            }));
            Ok(())
        };
        try_start_on(&slot, MIN_INTERVAL, build).unwrap();
        let old_run = slot.load_full().unwrap();
        let old_poller = old_run.poller.load_full().unwrap();
        assert!(request_stop_on(&slot, |poller| {
            assert!(std::ptr::eq(poller, old_poller.as_ref()));
            assert!(stop_join_on(&slot, |_| panic!("old STOP already committed")).unwrap());
            try_start_on(&slot, MAX_INTERVAL, build).unwrap();
            let new_run = slot.load_full().unwrap();
            assert!(!Arc::ptr_eq(&old_run, &new_run));
            let new_poller = new_run.poller.load_full().unwrap();
            assert!(!std::ptr::eq(poller, new_poller.as_ref()));
            notify(poller)?;
            assert_eq!(new_run.state.load(SeqCst) & (PHASE_MASK | STOP), ControlPhase::Owned as u8);
            Ok(())
        }).unwrap());
        let new_run = slot.load_full().unwrap();
        assert_eq!(new_run.interval, MAX_INTERVAL);
        assert_eq!(new_run.state.load(SeqCst) & STOP, 0);
        assert!(stop_join_on(&slot, notify).unwrap());
        assert!(slot.load_full().is_none());
    }

    /// 完成位与停止请求竞争（completion/stop race）同时经过真实原子出口（production atomic entries）；一次有界交错（bounded interleaving）不声称强制CAS失败（CAS failure）。
    // @trace 22
    #[test]
    fn test_concurrent_body_done_and_stop_preserve_one_state_word() {
        let slot = ControlSlot::empty();
        let run = create_run(DEFAULT_INTERVAL);
        run.poller.store(Some(Arc::new(Poller::new().unwrap())));
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let worker_barrier = barrier.clone();
        let worker_run = run.clone();
        let (done, completed) = oneshot::channel();
        let thread = std::thread::spawn(move || {
            let exit = WorkerExitGuard { run: &worker_run };
            worker_barrier.wait();
            drop(exit);
            done.send(()).unwrap();
            Ok(())
        });
        run.resources.store(Some(Arc::new(ResourceRecord::Owned(AtomicTake::new(OwnedRun {
            thread, control_error: None,
        })))));
        run.state.store(ControlPhase::Owned as u8, SeqCst);
        slot.store(Some(run.clone()));
        let notifications = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let requester = scope.spawn(|| {
                barrier.wait();
                request_stop_on(&slot, |poller| {
                    notifications.fetch_add(1, SeqCst);
                    notify(poller)
                })
            });
            barrier.wait();
            assert!(requester.join().unwrap().unwrap());
            completed.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        assert_eq!(run.state.load(SeqCst), ControlPhase::Owned as u8 | STOP | BODY_DONE);
        assert_eq!(notifications.load(SeqCst), 1);
        assert!(stop_join_on(&slot, |_| panic!("STOP already notified")).unwrap());
        assert!(slot.load_full().is_none());
        assert_eq!(run.state.load(SeqCst), ControlPhase::Collecting as u8 | STOP | BODY_DONE);
    }

    /// 退出未确认（exit unconfirmed）由合法恢复检查点（recovery checkpoint）进入；不强造原生join调用恐慌（native join-invocation panic）。
    /// 查询及停止请求（query/stop request）不消费诊断（diagnostic），仅首次回收（first collection）取得完整报告，后续仍拒绝重启（restart）。
    // @trace 23
    #[test]
    fn test_unconfirmed_exit_delivers_diagnostic_once_and_never_restarts() {
        let slot = ControlSlot::empty();
        let run = create_run(DEFAULT_INTERVAL);
        run.state.store(ControlPhase::Collecting as u8 | STOP | BODY_DONE, SeqCst);
        slot.store(Some(run.clone()));
        let mut guard = collection_guard(&slot, run.clone(), ControlPhase::Owned);
        guard.join_started = true;
        guard.control_error = Some(Error::new(ErrorKind::Io).attach_opaque(227_u32));
        drop(guard);
        assert_eq!(snapshot_on(&slot).phase, AutoRefreshPhase::ExitUnconfirmed);
        assert_eq!(request_stop_on(&slot, |_| panic!("unconfirmed notify")).unwrap_err().current_context(), &ErrorKind::Corrupted);
        for first in [true, false] {
            let error = stop_join_on(&slot, |_| panic!("unconfirmed collection notify")).unwrap_err();
            assert_eq!(error.current_context(), &ErrorKind::Corrupted);
            assert_eq!(error.downcast_ref::<u32>(), if first { Some(&227) } else { None });
            assert_eq!(error.downcast_ref::<ControlEffects>(), Some(&ControlEffects { stop_requested: false, worker_joined: false }));
            assert!(Arc::ptr_eq(&slot.load_full().unwrap(), &run));
            assert_eq!(try_start_on(&slot, MIN_INTERVAL, |_| panic!("unconfirmed restart build")).unwrap_err().current_context(), &ErrorKind::AlreadyExists);
        }
        assert_eq!(run.state.load(SeqCst), ControlPhase::ExitUnconfirmed as u8 | STOP | BODY_DONE);
    }

    /// 已确认join之后的转换展开（post-join conversion unwind）用安全恐慌负载（safe panic payload）的析构（drop）触发，不伪造join成功（join success）。
    /// 回收保护（collection guard）必须保留既有控制错误（control error）为Completed，随后交付一次完整结果（complete result）。
    // @trace 24
    #[test]
    fn test_joined_conversion_unwind_retains_completed_control_error() {
        struct PanicOnPayloadDrop;
        impl Drop for PanicOnPayloadDrop {
            /// 父线程（parent thread）释放已取回负载（collected payload）时展开一次；不在已有展开（active unwind）中触发。
            fn drop(&mut self) { panic!("payload-conversion-unwind"); }
        }
        let slot = ControlSlot::empty();
        let run = create_run(DEFAULT_INTERVAL);
        run.poller.store(Some(Arc::new(Poller::new().unwrap())));
        let worker_run = run.clone();
        let thread = std::thread::spawn(move || -> Result<()> {
            let _exit = WorkerExitGuard { run: &worker_run };
            std::panic::panic_any(PanicOnPayloadDrop);
        });
        run.resources.store(Some(Arc::new(ResourceRecord::Owned(AtomicTake::new(OwnedRun {
            thread, control_error: Some(Error::new(ErrorKind::Io).attach_opaque(229_u64)),
        })))));
        run.state.try_update(SeqCst, SeqCst, |old| {
            Some((old & !PHASE_MASK) | ControlPhase::Owned as u8)
        }).unwrap();
        slot.store(Some(run.clone()));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| stop_join_on(&slot, notify)));
        assert!(result.is_err());
        assert_eq!(run.state.load(SeqCst), ControlPhase::Completed as u8 | STOP | BODY_DONE);
        assert_eq!(snapshot_on(&slot).phase, AutoRefreshPhase::Finished);
        assert_eq!(try_start_on(&slot, DEFAULT_INTERVAL, |_| panic!("unclaimed Completed cannot restart")).unwrap_err().current_context(), &ErrorKind::AlreadyExists);
        let error = stop_join_on(&slot, |_| panic!("Completed must not notify or rejoin")).unwrap_err();
        assert_eq!(error.current_context(), &ErrorKind::Internal);
        assert_eq!(error.downcast_ref::<u64>(), Some(&229));
        assert_eq!(error.downcast_ref::<ControlEffects>(), Some(&ControlEffects { stop_requested: true, worker_joined: true }));
        assert!(slot.load_full().is_none());
        assert!(!stop_join_on(&slot, |_| panic!("already delivered")).unwrap());
    }
}
