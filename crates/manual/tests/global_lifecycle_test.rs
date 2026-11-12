//! 公开全局生命周期（public global lifecycle）在各自新进程（fresh process）运行。
//! 只经生产公开入口（production public entries）驱动，单个子进程（child process）限30秒。

use pi_clock_manual::global;
use pi_result::ErrorKind;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 用同一测试可执行文件（test executable）隔离进程级安装（process-level installation）；
/// 环境变量（environment variable）只选择本次入口，不改变产品状态（product state）。
fn in_fresh_process(test_name: &str) -> bool {
    if std::env::var("PI_CLOCK_U4_CHILD").ok().as_deref() == Some(test_name) {
        return true;
    }
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env("PI_CLOCK_U4_CHILD", test_name)
        .stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if child.try_wait().unwrap().is_some() { break; }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("子进程超时（child timeout）: {test_name}\n{output:?}");
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "子进程失败（child failure）: {test_name}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    false
}

// @trace 3
/// try_now未安装（uninitialized）为InvalidState；首装为零（zero），
/// 真实等待（real wait）及纯值运算（pure arithmetic）均不能自行刷新缓存（cache）。
#[test]
fn test_global_reads_require_init_and_remain_cached() {
    if !in_fresh_process("test_global_reads_require_init_and_remain_cached") { return; }
    assert_eq!(global::try_now().unwrap_err().current_context(), &ErrorKind::InvalidState);
    global::try_init().unwrap();
    assert_eq!(global::try_now().unwrap(), global::GlobalManualInstant::ZERO);
    std::thread::sleep(Duration::from_millis(1));
    global::GlobalManualInstant::ZERO.try_add(Duration::from_secs(1)).unwrap();
    assert_eq!(global::try_now().unwrap(), global::GlobalManualInstant::ZERO);
}

// @trace 4
/// 同步刷新（synchronous refresh）在未装时拒绝（reject）；首装后真实采样（real sampling）
/// 提高缓存（cache），结果与公开读数（public reading）一致，重复安装（duplicate installation）不归零。
#[test]
fn test_global_refresh_publishes_real_time_without_reset() {
    if !in_fresh_process("test_global_refresh_publishes_real_time_without_reset") { return; }
    assert_eq!(global::try_refresh().unwrap_err().current_context(), &ErrorKind::InvalidState);
    global::try_init().unwrap();
    std::thread::sleep(Duration::from_millis(1));
    let outcome = global::try_refresh().unwrap();
    assert!(outcome.sampled > global::GlobalManualInstant::ZERO);
    assert_eq!(outcome.published, outcome.sampled);
    assert!(outcome.advanced);
    assert_eq!(global::try_now().unwrap(), outcome.published);
    assert_eq!(global::try_init().unwrap_err().current_context(), &ErrorKind::AlreadyExists);
    assert_eq!(global::try_now().unwrap(), outcome.published);
    let copied = global::RefreshOutcome { published: global::GlobalManualInstant::ZERO, ..outcome };
    assert_ne!(copied.published, global::try_now().unwrap());
    std::thread::sleep(Duration::from_millis(1));
    assert_eq!(global::try_now().unwrap(), outcome.published);
}

/// 唤醒计数器（wake counter）只检测真实异步计算（Future）是否请求唤醒（wake-up）。
struct WakeCounter(std::sync::atomic::AtomicUsize);

impl std::task::Wake for WakeCounter {
    /// 只累计唤醒请求（wake requests），用于观察真实Future，不替它执行刷新（refresh）。
    fn wake(self: std::sync::Arc<Self>) {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}

// @trace 5
/// 真实try_refresh_async在创建／未轮询丢弃（creation/unpolled drop）时无副作用；
/// 未安装首轮询（first poll）直接Ready错误，安装前创建的对象按轮询时状态（poll-time state）成功。
#[test]
fn test_global_async_refresh_is_lazy_ready_and_send() {
    if !in_fresh_process("test_global_async_refresh_is_lazy_ready_and_send") { return; }
    use std::future::Future;
    use std::pin::pin;
    use std::task::{Context, Poll, Waker};
    use std::sync::atomic::Ordering::SeqCst;

    fn assert_send<T: Send>(_: &T) {}
    let wakes = std::sync::Arc::new(WakeCounter(std::sync::atomic::AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let mut context = Context::from_waker(&waker);
    drop(global::try_refresh_async());
    assert_eq!(global::try_now().unwrap_err().current_context(), &ErrorKind::InvalidState);
    let mut uninitialized = pin!(global::try_refresh_async());
    assert_send(&uninitialized);
    match uninitialized.as_mut().poll(&mut context) {
        Poll::Ready(Err(error)) => assert_eq!(error.current_context(), &ErrorKind::InvalidState),
        _ => panic!("未安装首次轮询必须就绪报错（uninitialized first poll must be ready with error）"),
    }
    let mut created_before_init = pin!(global::try_refresh_async());
    assert_send(&created_before_init);
    global::try_init().unwrap();
    assert_eq!(global::try_now().unwrap(), global::GlobalManualInstant::ZERO);
    std::thread::sleep(Duration::from_millis(1));
    drop(global::try_refresh_async());
    assert_eq!(global::try_now().unwrap(), global::GlobalManualInstant::ZERO);
    let outcome = match created_before_init.as_mut().poll(&mut context) {
        Poll::Ready(Ok(outcome)) => outcome,
        _ => panic!("安装后首次轮询必须就绪成功（initialized first poll must be ready with success）"),
    };
    assert!(outcome.sampled > global::GlobalManualInstant::ZERO);
    assert_eq!(outcome.sampled, outcome.published);
    assert!(outcome.advanced);
    assert_eq!(global::try_now().unwrap(), outcome.published);
    assert_eq!(wakes.0.load(SeqCst), 0);
}

// @trace 6
/// 两个写者（writers）和两个读者（readers）各执行8次公开操作（public operations），
/// 每轮同步进入（synchronized start）但不限定竞争胜者（race winner）；读数及写入描述（publication description）各自非递减。
#[test]
fn test_global_multiwriter_multireader_values_never_regress() {
    if !in_fresh_process("test_global_multiwriter_multireader_values_never_regress") { return; }
    global::try_init().unwrap();
    let rounds = std::sync::Barrier::new(4);
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for index in 0..4 {
            let rounds_ref = &rounds;
            handles.push(scope.spawn(move || {
                let mut previous = global::GlobalManualInstant::ZERO;
                let mut observations = Vec::new();
                for _ in 0..8 {
                    rounds_ref.wait();
                    let observed = if index < 2 {
                        let outcome = global::try_refresh().unwrap();
                        assert!(outcome.published >= outcome.sampled);
                        if outcome.advanced { assert_eq!(outcome.published, outcome.sampled); }
                        outcome.published
                    } else {
                        global::try_now().unwrap()
                    };
                    assert!(observed >= previous);
                    previous = observed;
                    observations.push(observed);
                }
                observations
            }));
        }
        let observations: Vec<_> = handles.into_iter().map(|handle| handle.join().unwrap()).collect();
        let published: Vec<_> = observations[..2].iter().flatten().copied().collect();
        for value in observations[2..].iter().flatten() {
            assert!(*value == global::GlobalManualInstant::ZERO || published.contains(value));
        }
        assert_eq!(global::try_now().unwrap(), *published.iter().max().unwrap());
    });
}

// @trace 7
/// 能力查询（capability query）未安装（uninitialized）时明确失败；
/// 安装后与真实平台公开来源（public platform source）一致，多次查询（queries）不刷新缓存（cache）。
#[test]
fn test_global_capabilities_are_fixed_and_do_not_advance_time() {
    if !in_fresh_process("test_global_capabilities_are_fixed_and_do_not_advance_time") { return; }
    use pi_clock_core::source::MonotonicSource;
    assert_eq!(global::try_capabilities().unwrap_err().current_context(), &ErrorKind::InvalidState);
    global::try_init().unwrap();
    let expected = pi_clock_platform::monotonic::PlatformMonotonicSource::try_prepare().unwrap().capabilities();
    let first = global::try_capabilities().unwrap();
    assert_eq!(first, expected);
    assert_eq!(global::try_capabilities().unwrap(), first);
    assert_eq!(global::try_now().unwrap(), global::GlobalManualInstant::ZERO);
    std::thread::sleep(Duration::from_millis(1));
    assert_eq!(global::try_capabilities().unwrap(), first);
    assert_eq!(global::try_now().unwrap(), global::GlobalManualInstant::ZERO);
    let mut copied = first;
    copied.suspend = pi_clock_core::source::SuspendBehavior::Unspecified;
    assert_eq!(copied.suspend, pi_clock_core::source::SuspendBehavior::Unspecified);
    assert_eq!(global::try_capabilities().unwrap(), expected);
}

// @trace 1
/// 验证try_init公开安装（public installation）成功一次，后续返回AlreadyExists。
#[test]
fn test_global_init_is_unique() {
    if !in_fresh_process("test_global_init_is_unique") { return; }
    global::try_init().unwrap();
    assert_eq!(global::try_init().unwrap_err().current_context(), &ErrorKind::AlreadyExists);
}

// @trace 2
/// 两线程（two threads）同时从公开try_init竞争，唯一成功（single success）；
/// 使用最小候选数（minimum contender count），不作压力测试（stress testing）。
#[test]
fn test_global_init_concurrent_unique() {
    if !in_fresh_process("test_global_init_concurrent_unique") { return; }
    let gate = std::sync::Barrier::new(3);
    std::thread::scope(|scope| {
        let first = scope.spawn(|| { gate.wait(); global::try_init() });
        let second = scope.spawn(|| { gate.wait(); global::try_init() });
        gate.wait();
        let results = [first.join().unwrap(), second.join().unwrap()];
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.into_iter().find_map(|result| result.err()).unwrap().current_context(), &ErrorKind::AlreadyExists);
    });
}
