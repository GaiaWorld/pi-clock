//! 自动推进公开生命周期（automatic-refresh public lifecycle）使用新进程（fresh process）和真实Poller/global。
//! 每个子进程（child process）有30秒截止（deadline），只通过公开入口（public entries）操作。
#![cfg(all(feature = "auto-refresh", any(target_os = "windows", target_os = "linux", target_os = "android")))]

use pi_clock_manual::{auto_refresh, global};
use pi_result::ErrorKind;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 进程隔离（process isolation）只选择测试入口（test entry），不复位（reset）或注入生产状态（production state）。
fn in_fresh_process(name: &str) -> bool {
    if std::env::var("PI_CLOCK_U5_CHILD").ok().as_deref() == Some(name) { return true; }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env("PI_CLOCK_U5_CHILD", name).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() { break; }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("子进程超时（child timeout）: {name} {output:?}");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{name}
{}
{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    false
}

/// 启动（startup）要求显式核心初始化（explicit core initialization），失败附本次零控制效果（zero control effects）。
// @trace 1
#[test]
fn test_uninitialized_start_preserves_core() {
    if !in_fresh_process("test_uninitialized_start_preserves_core") { return; }
    let error = auto_refresh::try_start(None).unwrap_err();
    assert_eq!(error.current_context(), &ErrorKind::InvalidState);
    assert_eq!(error.downcast_ref::<auto_refresh::ControlEffects>(),
        Some(&auto_refresh::ControlEffects { stop_requested: false, worker_joined: false }));
    assert_eq!(global::try_now().unwrap_err().current_context(), &ErrorKind::InvalidState);
}

/// 状态查询（status query）未启动为Stopped且不隐式安装核心（implicit core installation）。
// @trace 2
#[test]
fn test_status_is_observational() {
    if !in_fresh_process("test_status_is_observational") { return; }
    assert_eq!(auto_refresh::status(), auto_refresh::AutoRefreshStatus {
        phase: auto_refresh::AutoRefreshPhase::Stopped, interval: None,
    });
    assert_eq!(global::try_now().unwrap_err().current_context(), &ErrorKind::InvalidState);
}

/// 空槽停止（empty-slot stop）返回false，不安装核心（core）或启动线程（thread）。
// @trace 3
#[test]
fn test_empty_stop_request_has_no_effect() {
    if !in_fresh_process("test_empty_stop_request_has_no_effect") { return; }
    assert!(!auto_refresh::try_request_stop().unwrap());
    assert_eq!(auto_refresh::status().phase, auto_refresh::AutoRefreshPhase::Stopped);
    assert_eq!(global::try_now().unwrap_err().current_context(), &ErrorKind::InvalidState);
}

/// 公开回收（public collection）空槽幂等（idempotent），不隐式安装核心（core installation）。
// @trace 4
#[test]
fn test_empty_stop_join_has_no_effect() {
    if !in_fresh_process("test_empty_stop_join_has_no_effect") { return; }
    assert!(!auto_refresh::try_stop_and_join().unwrap());
    assert!(!auto_refresh::try_stop_and_join().unwrap());
    assert_eq!(global::try_now().unwrap_err().current_context(), &ErrorKind::InvalidState);
}

/// 完整公开生命周期（complete public lifecycle）：先等待（wait first）、停止/回收（stop/collect）、手动刷新（manual refresh）及重启同时间轴（same-timeline restart）。
// @trace 5
#[test]
fn test_real_lifecycle_waits_first_and_restarts_same_timeline() {
    if !in_fresh_process("test_real_lifecycle_waits_first_and_restarts_same_timeline") { return; }
    global::try_init().unwrap();
    let started = Instant::now();
    assert_eq!(auto_refresh::try_start(Some(Duration::from_secs(1))).unwrap(), Duration::from_secs(1));
    assert!(started.elapsed() < Duration::from_secs(1), "观察窗口已被调度耗尽（observation window exhausted by scheduling）");
    assert_eq!(global::try_now().unwrap(), global::GlobalManualInstant::ZERO);
    assert_eq!(auto_refresh::status(), auto_refresh::AutoRefreshStatus {
        phase: auto_refresh::AutoRefreshPhase::Running, interval: Some(Duration::from_secs(1)),
    });
    let error = auto_refresh::try_start(None).unwrap_err();
    assert_eq!(error.current_context(), &ErrorKind::AlreadyExists);
    assert_eq!(error.downcast_ref::<auto_refresh::ControlEffects>(),
        Some(&auto_refresh::ControlEffects { stop_requested: false, worker_joined: false }));
    assert!(auto_refresh::try_request_stop().unwrap());
    assert!(!auto_refresh::try_request_stop().unwrap());
    assert!(auto_refresh::try_stop_and_join().unwrap());
    assert_eq!(auto_refresh::status().phase, auto_refresh::AutoRefreshPhase::Stopped);
    assert_eq!(global::try_now().unwrap(), global::GlobalManualInstant::ZERO);
    let manual = global::try_refresh().unwrap().published;
    assert!(manual > global::GlobalManualInstant::ZERO);
    auto_refresh::try_start(Some(Duration::from_millis(2))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut previous = manual;
    loop {
        let current = global::try_now().unwrap();
        assert!(current >= previous);
        if current > manual { break; }
        previous = current;
        assert!(Instant::now() < deadline, "真实自动刷新未推进（real automatic refresh did not advance）");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(auto_refresh::try_stop_and_join().unwrap());
    let retained = global::try_now().unwrap();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(global::try_now().unwrap(), retained);
    assert!(!auto_refresh::try_stop_and_join().unwrap());
    assert_eq!(global::try_init().unwrap_err().current_context(), &ErrorKind::AlreadyExists);
    assert!(global::try_refresh().unwrap().published >= retained);
}

/// 间隔合法域（interval domain）从公开入口（public entry）验证，每次回收（collect）后才启动下一周期（next run）。
// @trace 6
#[test]
fn test_public_interval_normalization() {
    if !in_fresh_process("test_public_interval_normalization") { return; }
    global::try_init().unwrap();
    for (input, expected) in [
        (None, Duration::from_millis(1)),
        (Some(Duration::ZERO), Duration::from_millis(1)),
        (Some(Duration::from_nanos(99_999)), Duration::from_millis(1)),
        (Some(Duration::from_micros(100)), Duration::from_micros(100)),
        (Some(Duration::from_nanos(100_001)), Duration::from_nanos(100_001)),
        (Some(Duration::from_secs(1)), Duration::from_secs(1)),
        (Some(Duration::new(1, 1)), Duration::from_millis(1)),
        (Some(Duration::MAX), Duration::from_millis(1)),
    ] {
        assert_eq!(auto_refresh::try_start(input).unwrap(), expected);
        assert_eq!(auto_refresh::status().interval, Some(expected));
        assert!(auto_refresh::try_stop_and_join().unwrap());
    }
}

/// 两个启动者（two starters）同时调用公开入口（public entry），只有一个成功（single winner），败者不覆盖运行（run）。
// @trace 7
#[test]
fn test_public_concurrent_start_has_one_winner() {
    if !in_fresh_process("test_public_concurrent_start_has_one_winner") { return; }
    global::try_init().unwrap();
    let barrier = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| { barrier.wait(); auto_refresh::try_start(Some(Duration::from_secs(1))) });
        let second = scope.spawn(|| { barrier.wait(); auto_refresh::try_start(Some(Duration::from_secs(1))) });
        barrier.wait();
        let results = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.into_iter().find_map(|result| result.err()).unwrap().current_context(), &ErrorKind::AlreadyExists);
    });
    assert!(auto_refresh::try_stop_and_join().unwrap());
    assert_eq!(auto_refresh::status().phase, auto_refresh::AutoRefreshPhase::Stopped);
}

/// 两个公开回收者（public collectors）同时调用；唯一胜者（single winner）确认退出，另一调用只允许Busy或已清槽false（already-cleared slot）。
/// 真实线程调度（thread scheduling）不保证观察到Busy；确定的Collecting拒绝（Collecting rejection）由私有协议测试（private-protocol test）另证。
// @trace 8
#[test]
fn test_public_concurrent_collection_delivers_once() {
    if !in_fresh_process("test_public_concurrent_collection_delivers_once") { return; }
    global::try_init().unwrap();
    auto_refresh::try_start(Some(Duration::from_secs(1))).unwrap();
    let barrier = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| { barrier.wait(); auto_refresh::try_stop_and_join() });
        let second = scope.spawn(|| { barrier.wait(); auto_refresh::try_stop_and_join() });
        barrier.wait();
        let mut delivered = 0;
        for result in [first.join().unwrap(), second.join().unwrap()] {
            match result {
                Ok(joined) => delivered += usize::from(joined),
                Err(error) => {
                    assert_eq!(error.current_context(), &ErrorKind::Busy);
                    assert_eq!(error.downcast_ref::<auto_refresh::ControlEffects>(), Some(&auto_refresh::ControlEffects {
                        stop_requested: false, worker_joined: false,
                    }));
                }
            }
        }
        assert_eq!(delivered, 1);
    });
    assert_eq!(auto_refresh::status(), auto_refresh::AutoRefreshStatus {
        phase: auto_refresh::AutoRefreshPhase::Stopped, interval: None,
    });
    assert!(!auto_refresh::try_stop_and_join().unwrap());
}
