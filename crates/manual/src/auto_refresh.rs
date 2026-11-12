//! 全局手推时钟的可选自动刷新（optional automatic refresh）。
//!
//! 先显式调用crate::global::try_init，再启动唯一原生线程（native thread）。
//! None或超出100µs至1s的间隔采用1ms；第一轮先等待，再真实刷新。
//! 本库共享控制状态（shared control state）采用原子操作；普通global读取不进入本模块。
//! 启动握手（startup handshake）、内核等待（kernel wait）和线程回收（thread join）按语义阻塞，
//! 不能把这些完整操作称为无阻塞（nonblocking）或承诺固定延迟（bounded latency）。
//! 停止自动刷新不关闭时钟；失败后显式回收完整pi_result结果再重启。
//! Windows不足1ms的正超时向上取整为1ms；不保证硬实时（hard real-time）。
//! ExitUnconfirmed保留唯一资格（single qualification），禁止强制复位或另起线程。
//!
//! ```no_run
//! # fn example() -> pi_result::Result<()> {
//! use pi_clock_manual::{auto_refresh, global};
//! global::try_init()?;
//! let interval = auto_refresh::try_start(None)?;
//! let now = global::try_now()?;
//! let collected = auto_refresh::try_stop_and_join()?;
//! # let _ = (interval, now, collected);
//! # Ok(())
//! # }
//! ```

use arc_swap::ArcSwapOption;
use atomic_take::AtomicTake;
use pi_result::{Error, Result};
use polling::{Events, Poller};
use std::sync::atomic::AtomicU8;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 默认刷新间隔（default refresh interval）：1ms。
pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(1);
/// 最小有效间隔（minimum valid interval）：100µs，包含端点。
pub const MIN_INTERVAL: Duration = Duration::from_micros(100);
/// 最大有效间隔（maximum valid interval）：1s，包含端点。
pub const MAX_INTERVAL: Duration = Duration::from_secs(1);

/// 自动刷新阶段（automatic refresh phase）；返回副本不提供控制权限。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AutoRefreshPhase {
    /// 尚未启动或上次已回收（stopped/reaped）。
    Stopped,
    /// 已取得资格，等待准备和就绪（starting）。
    Starting,
    /// 已就绪，尚未确认停止请求（running）。
    Running,
    /// 已请求停止，线程可能仍完成已进入的一轮（stop requested）。
    StopRequested,
    /// 主体已结束或结果待回收（finished）；不等同线程局部析构已完成。
    Finished,
    /// 另一个调用正在停止并回收（joining）。
    Joining,
    /// 无法确认退出（exit unconfirmed），保留资格；不允许强制复位或重启。
    ExitUnconfirmed,
}

/// 某次查询期间成立过的状态快照（status snapshot）；可能在返回前改变。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AutoRefreshStatus {
    /// 自动刷新阶段（automatic refresh phase）。
    pub phase: AutoRefreshPhase,
    /// 本周期实际间隔（effective interval）；Stopped时为None。
    pub interval: Option<Duration>,
}

/// 错误报告（error report）中的本次控制效果（control effects）。
///
/// 通过报告的downcast_ref读取；只描述本次选择的运行，不授予权限或指认当前周期。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ControlEffects {
    /// 本次选择的运行已确认存在停止请求（stop request）。
    pub stop_requested: bool,
    /// 已确认该运行的线程退出（worker joined），即使最终返回错误。
    pub worker_joined: bool,
}

// 内部设计（internal design）：下面不是公开控制权限或API文档。
// 计时核心（clock core）仍由global唯一拥有；本槽只定位唯一自动运行。
// 不使用Mutex/RwLock、自旋锁、UnsafeCell或本库自写unsafe。
// ArcSwap用默认策略且不启用rwlock-strategy；原子同步不保证分配/析构/内核操作无阻塞。
// 原子发布（atomic publication）与唯一移动（single ownership transfer）是不同义务。
// AtomicTake的Relaxed交换只裁定唯一接收方；资源可见性由先发布资源、后SeqCst提交阶段建立。
// 每周期Arc标识全新；持有旧Arc时不得重用其身份或以裸地址重建权限。
const PHASE_MASK: u8 = 0b0000_0111;
const STOP: u8 = 0b0000_1000;
const BODY_DONE: u8 = 0b0001_0000;

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
enum ControlPhase {
    Starting = 0,
    Owned = 1,
    Collecting = 2,
    Completed = 3,
    ExitUnconfirmed = 4,
}

struct OwnedRun {
    thread: JoinHandle<Result<()>>,
    control_error: Option<Error>,
}

enum ResourceRecord {
    Owned(AtomicTake<OwnedRun>),
    Completed(AtomicTake<Result<()>>),
    Unconfirmed(AtomicTake<Error>),
}

struct RunControl {
    interval: Duration,
    state: AtomicU8,
    poller: ArcSwapOption<Poller>,
    resources: ArcSwapOption<ResourceRecord>,
}

type ControlSlot = ArcSwapOption<RunControl>;
static CONTROL: ControlSlot = ArcSwapOption::const_empty();

// 无析构身份标记（non-dropping identity marker）；自身回收先于Busy拒绝。
std::thread_local! {
    static AUTO_WORKER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct StartupGuard<'a> {
    slot: &'a ControlSlot,
    run: Arc<RunControl>,
    thread: Option<JoinHandle<Result<()>>>,
    staged: Option<Arc<ResourceRecord>>,
    armed: bool,
}

struct CollectionGuard<'a> {
    slot: &'a ControlSlot,
    run: Arc<RunControl>,
    original: Option<Arc<ResourceRecord>>,
    origin: ControlPhase,
    thread: Option<JoinHandle<Result<()>>>,
    outcome: Option<Result<()>>,
    control_error: Option<Error>,
    notify_needed: bool,
    join_started: bool,
    worker_joined: bool,
    armed: bool,
}

struct WorkerExitGuard<'a> {
    run: &'a RunControl,
}

/// 同步启动（synchronous startup），返回实际间隔（effective interval）。
/// None或超界采用1ms；端点有效。先显式初始化global，重复启动AlreadyExists；成功不保证返回瞬间仍运行。
// 内部约束（internal obligations）：先global::try_capabilities验证显式安装，不隐式初始化/刷新；normalize_interval后调用try_start_on及spawn_ready。就绪成功仅说明线程曾准备完成；第一轮仍先完整等待。全部公开Err附ControlEffects；普通准备错误原样保留，可回收清理后重试。
// @trace 1
pub fn try_start(interval: Option<Duration>) -> Result<Duration> {
    start_global_inner(interval)
}

/// 查询可成立的状态快照（status snapshot）。
/// 不采样、不消费结果、不取得互斥锁。BODY_DONE只表示主体结束，完整退出须join。
// 内部约束（internal obligations）：只调用snapshot_on，不采样、不取走退出结果、不修改状态。快照只保证本次查询期间成立过，不能代替下一控制操作的资格检查；Finished不等同TLS析构结束。
// @trace 2
pub fn status() -> AutoRefreshStatus {
    status_global_inner()
}

/// 提交停止请求（stop request），不等待退出（exit）。
/// 首次请求返回true，重复/空槽false；Starting Busy。通知失败请求仍生效，错误附ControlEffects。
// 内部约束（internal obligations）：使用CONTROL与本模块notify调用request_stop_on；返回是否首次提交。允许已经进入的一轮刷新完成；保留pi_result及ControlEffects，不追加启动/隐式回收。
// @trace 3
pub fn try_request_stop() -> Result<bool> {
    request_stop_global_inner()
}

/// 停止并等待退出（stop and join），唯一取得完整结果（complete result）。
/// 空槽false，实际回收true；并发回收Busy，自身等待先InvalidState。通知失败仍回收且保留同次所有错误。
// 内部约束（internal obligations）：首先AUTO_WORKER标记拒绝self-join为InvalidState且无副作用，优先于Busy；再调用stop_join_on。空槽false，实际回收true；通知失败仍join并合并同次完整错误。worker_joined=true的返回错误也表示已确认退出且本次结果已交付，可显式重启。这是同步阻塞入口，不在要求不阻塞的任务执行上下文直接调用。
// @trace 4
pub fn try_stop_and_join() -> Result<bool> {
    stop_join_global_inner()
}

// # 用途
// 完整时长规范化（duration normalization）。
// 内部约束（internal obligations）：None或interval小于100µs/大于1s返回1ms，端点原样保留。完整Duration比较，含0及Duration::MAX；不夹紧、不溢出、不取时或分配。
// @trace 5
fn normalize_interval(interval: Option<Duration>) -> Duration {
    normalize_interval_inner(interval)
}

// # 用途
// 同一生产唯一启动协议（single-start protocol）。
// 内部约束（internal obligations）：先观察非空槽拒绝AlreadyExists且不调用build；空槽调用create_run并CAS None->本Arc，竞争败者释放空候选且不调用build。成功后StartupGuard拥有Starting资格，锁外build完成资源/线程就绪。成功先把完整Owned资源发布到run.resources，再保留STOP/BODY_DONE以CAS提交Owned，解除保护。普通失败未生成线程则仅CAS本Arc->None；已有线程必须CAS Starting->Collecting|STOP，同时从成功旧字计算notify_needed，转交完整句柄/错误给已armed的CollectionGuard再解除原保护，统一collect_owned。任何可重试CAS闭包只处理固定值，不放build/通知/采样/分配。staged资源保证提交前展开不丢句柄；提交Owned后的旧StartupGuard不得再取回资源。
// @trace 6
fn try_start_on<'a, F>(slot: &'a ControlSlot, interval: Duration, build: F) -> Result<Duration>
where F: FnOnce(&mut StartupGuard<'a>) -> Result<()> {
    start_on_inner(slot, interval, build)
}

// # 用途
// 一致状态投影（consistent state projection）。
// 内部约束（internal obligations）：load_full取得本Arc，单次SeqCst加载state，再load_full确认仍同一Arc，否则重新观察；None可直接返回Stopped/None。同次状态字投影Starting、Owned派生Running/StopRequested/Finished（BODY_DONE优先）、Collecting为Joining、Completed为Finished、ExitUnconfirmed。interval取同一不可变Arc。不得拼接独立原子字段或把快照当操作权限；不消费AtomicTake，不调用JoinHandle::is_finished，不取内核时间。
// @trace 7
fn snapshot_on(slot: &ControlSlot) -> AutoRefreshStatus {
    snapshot_on_inner(slot)
}

// # 用途
// 停止原子发布（atomic stop publication）及通知（notification）。
// 内部约束（internal obligations）：load_full固定本周期；空槽false，Starting Busy，Collecting/Completed false，ExitUnconfirmed Corrupted且不消费诊断。Owned若STOP已有则false；否则CAS同一state只置STOP，保留阶段及BODY_DONE。只成功CAS的首次请求调用一次notify；若CAS见阶段变化按新阶段返回或见BODY_DONE保留重试，不修改别人资格。notify在CAS之后且重试闭包之外；失败不回滚，附ControlEffects(true,false)。旧Arc延后通知只作用本运行。未取得运行或未执行控制的错误不得虚构本次效果。
// @trace 8
fn request_stop_on<N>(slot: &ControlSlot, notify: N) -> Result<bool>
where N: FnOnce(&Poller) -> Result<()> {
    request_stop_on_inner(slot, notify)
}

// # 用途
// 同一生产唯一回收协议（single-collector protocol）。
// 内部约束（internal obligations）：先自身标记拒绝，空槽false；Starting/Collecting Busy。准备original=None且记录origin的未armed CollectionGuard；Owned用一次SeqCst CAS同时提交Collecting|STOP并授予唯一回收权，保持BODY_DONE，notify_needed由成功旧STOP计算；成功后立即armed，随后才load_full当前run.resources并从Owned记录AtomicTake一次移交完整句柄/控制错误；不得把CAS前预读资源跨Owned/Collecting/Owned阶段往返使用，不留可恐慌的未受保护转移窗口。败者不置STOP、不取资源、不调用notify。Completed同样先CAS唯一资格，成功后才加载当前记录并取走已有结果，worker_joined=true，不再join；ExitUnconfirmed只以AtomicTake一次领取完整诊断，以后仍Corrupted，永不清槽。胜者统一调用collect_owned；未知/缺失内部资源必须保留资格并报告Corrupted，不能伪造成功。
// @trace 9
fn stop_join_on<N>(slot: &ControlSlot, notify: N) -> Result<bool>
where N: FnOnce(&Poller) -> Result<()> {
    stop_join_on_inner(slot, notify)
}

// # 用途
// 构造空运行记录（empty run record）。
// 内部约束（internal obligations）：interval已规范化；构造全新Arc，state为Starting且STOP/BODY_DONE清零，poller/resources为空。只分配运行记录，不创建内核资源、线程或通道，不采样global，不分配外部身份。Arc分配失败不承诺可恢复pi_result错误；不把系统取时放此路径。
// @trace 10
fn create_run(interval: Duration) -> Arc<RunControl> {
    create_run_inner(interval)
}

// # 用途
// 原生资源准备与就绪握手（native preparation/readiness handshake）。
// 内部约束（internal obligations）：本保护拥有Starting。Poller::new错误按pi_result IO分类保留；成功Arc发布到run.poller后创建oneshot::channel<()>，仅启用std、不启用async。Builder固定无NUL线程名创建worker；recv前JoinHandle存入attempt。recv为已确认的同步阻塞，断连失败交由启动协议统一停止/join；不detach。收到ready后线程随后失败不否定已完成握手。就绪通道只一次创建/发送/接收，不能进入每轮刷新。
// @trace 11
fn spawn_ready(attempt: &mut StartupGuard<'_>) -> Result<()> {
    spawn_ready_inner(attempt)
}

// # 用途
// 单一工作线程（worker thread），独占内核等待（kernel waiting）。
// 内部约束（internal obligations）：首先设置AUTO_WORKER，然后立即安装WorkerExitGuard，覆盖正常返回与panic展开。从run.poller一次load_full取得固定Poller，创建Events后发送ready，失败Cancelled退出；随后调用run_loop(&run.state,interval)，等待闭包仅wait_until，刷新闭包仅global::try_refresh映射单位值。工作循环不读全局CONTROL或run.resources，不逐轮Arc复制。退出保护发布BODY_DONE只是主体结束，不代替join。
// @trace 12
fn worker(run: Arc<RunControl>, ready: oneshot::Sender<()>) -> Result<()> {
    worker_inner(run, ready)
}

// # 用途
// 固定延迟循环（fixed-delay loop）。
// 内部约束（internal obligations）：每轮SeqCst检查STOP；用真实std::Instant::now.checked_add(interval)构造期限，失败ResourceExhausted且不进入Waiting。每次等待前后和刷新前检查STOP；空返回未到期保持原deadline，不能视为到期。wait闭包只在原始Interrupted处理后返回，本层不把pi_result Cancelled当Interrupted。到期且未停止仅一次refresh；其他错误原样退出，成功才开始下一完整间隔，不补跑、不长期忙等。W/R为同一私有生产算法的可控验证接入，不是公开运行时trait。
// @trace 13
fn run_loop<W, R>(state: &AtomicU8, interval: Duration, wait: W, refresh: R) -> Result<()>
where W: FnMut(Instant) -> Result<()>, R: FnMut() -> Result<()> {
    run_loop_inner(state, interval, wait, refresh)
}

// # 用途
// 单一内核期限等待（single kernel deadline wait）。
// 内部约束（internal obligations）：clear并复用Events，调用Poller::wait_deadline；仅原始std::io::ErrorKind::Interrupted继续同deadline，其余IO错误经pi_result保留。事件数量不是到期证明；只有工作线程可调用wait，不注册用户FD、不新增unsafe，不sleep轮询或零超时补跑。
// @trace 14
fn wait_until(poller: &Poller, events: &mut Events, deadline: Instant) -> Result<()> {
    wait_until_inner(poller, events, deadline)
}

// # 用途
// 最佳努力通知（best-effort notification）。
// 内部约束（internal obligations）：仅调用一次Poller::notify并保留IO错误；STOP及控制资格在调用前已原子提交，通知不改变时间/资格。失败后的再次notify不一定重发，Ok不证明线程已处理；不在可重试CAS闭包内通知。
// @trace 15
fn notify(poller: &Poller) -> Result<()> {
    notify_inner(poller)
}

// # 用途
// 完整报告合并（complete report combination）。
// 内部约束（internal obligations）：无错误Ok，单个原样，多个以error-stack的expand/push/change_context保留所有frames及阶段附件，恢复首要ErrorKind。线程错误优先，通知/启动清理错误次要。不可Clone报告，不只拼字符串，不吞掉任一分支；仅错误路径分配。
// @trace 16
fn merge_errors(primary: Option<Error>, secondary: Option<Error>) -> Result<()> {
    merge_errors_inner(primary, secondary)
}

// # 用途
// 已确认退出结果转换（confirmed-exit conversion）。
// 内部约束（internal obligations）：join外层Ok透传线程pi_result结果；外层Err为已确认线程panic，提取str/String或非字符串说明构造Internal，原panic负载按所有权释放。join返回的panic结果不同于join调用自身展开；不捕获abort/进程退出，不公开Box<Any+Send>。调用前guard必须已标记worker_joined=true。
// @trace 17
fn finish_join(raw: std::thread::Result<Result<()>>) -> Result<()> {
    finish_join_inner(raw)
}

// # 用途
// 共享启动失败及正常退出回收（shared exit collection）。
// 内部约束（internal obligations）：前提为guard已唯一占有本Arc的Collecting且STOP已置；原保护已解除或资源仍由guard.original保护。只notify_needed时通知，失败合并control_error仍继续join。join前join_started=true，返回后立即worker_joined=true，再finish_join并保存outcome；已有Completed结果不再join。合并完整结果及ControlEffects(true,true)，先把本次结果安全移到返回对象，再CAS全局本Arc->None并解除保护，最后交付；本次清理不得清其他Arc。CAS/组装阶段若展开守卫恢复完整资源/结果；常规错误也是已交付结果，允许后续显式重启。
// @trace 18
fn collect_owned<N>(guard: CollectionGuard<'_>, notify: N) -> Result<bool>
where N: FnOnce(&Poller) -> Result<()> {
    collect_owned_inner(guard, notify)
}

// # 用途
// 启动展开恢复（startup unwind recovery）。
// 内部约束（internal obligations）：未armed直接结束。若本Arc尚Starting且无线程/无staged句柄，只CAS本Arc->None；若已有线程，先CAS Starting->Collecting|STOP并从成功旧字取得notify_needed，在本保护仍持有该排他资格时从thread或staged记录接管完整句柄，执行必要notify并合并错误，然后先发布完整Owned资源再CAS提交Owned（STOP保持、BODY_DONE保留）。不能先公开Owned再补通知错误，否则会与另一回收者竞争。若已经提交Owned/Collecting，原保护不再取回资源，不能抢走别人的句柄或结果。Drop不直接join、不detach、不清他人资格；恢复失败保留本Arc并以退出未确认拒绝重启。异常Drop不能在已有展开期间二次恐慌。
// @trace 19
impl Drop for StartupGuard<'_> {
    /// # 用途
    /// 启动展开恢复（startup unwind recovery）。
    fn drop(&mut self) {
        drop_startup_inner(self)
    }
}

// # 用途
// 回收展开恢复（collection unwind recovery）。
// 内部约束（internal obligations）：未armed直接结束。只恢复本Arc且当前Collecting归本保护：优先按guard本地完整thread/outcome恢复；只有本地无thread/outcome且尚未从Owned/Completed领取时，original尚None才在Collecting下加载当前resources。origin=Starting且本地已有句柄、resources仍None是合法启动清理，不能误报缺失。依据origin与实际记录恢复；原Owned记录尚未take则保留原记录恢复Owned，原Completed记录尚未take则保留真实结果恢复Completed，不能用Internal替换未领取的报告。已取得且仍有句柄则先发布完整Owned记录再提交Owned。worker_joined且已有outcome时先发布Completed记录再提交Completed；只有真实join后转换意外展开、outcome确未产生时才用Internal。发布Completed必须先把guard.control_error完整合并进outcome或Internal；发布Unconfirmed也必须合并既有通知/启动错误，不能让无独立control_error字段的记录丢弃这些报告。join已开始、句柄已消耗且未确认则先发布Unconfirmed完整诊断再提交ExitUnconfirmed；无资格释放。状态CAS保留并发BODY_DONE且STOP永不清零；资源在阶段提交前发布，不把被取空的旧记录重新当完整资源。不得二次恐慌、join、重启或清他人Arc。
// @trace 20
impl Drop for CollectionGuard<'_> {
    /// # 用途
    /// 回收展开恢复（collection unwind recovery）。
    fn drop(&mut self) {
        drop_collection_inner(self)
    }
}

// # 用途
// 工作主体完成发布（worker-body completion publication）。
// 内部约束（internal obligations）：工作线程入口立即装入本保护，正常返回/可展开panic都以SeqCst fetch_or(BODY_DONE)发布一次；保留STOP及阶段位，不触碰全局槽/资源，不分配、不notify、不join、不加锁。BODY_DONE不证明TLS析构完成，正常因panic返回join仍须完整回收；abort不保证Drop运行。
// @trace 21
impl Drop for WorkerExitGuard<'_> {
    /// # 用途
    /// 工作主体完成发布（worker-body completion publication）。
    fn drop(&mut self) {
        drop_worker_inner(self)
    }
}

include!("auto_refresh_inner.rs");
