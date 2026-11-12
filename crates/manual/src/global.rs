//! 进程唯一的手推单调时间（process-global manually refreshed monotonic time）。
//!
//! 先调用 [`try_init`] 显式初始化（explicit initialization），再通过 [`try_now`]
//! 读取最近发布的缓存（published cache）。只有显式刷新（explicit refresh）成功采用
//! 更大样本时，读数才会前进；两次读取之间经过真实时间不会自行改变缓存。
//! 首个有效样本（first valid sample）是固定起点（fixed origin），初始读数为零。
//!
//! 本模块（module）适用于所有调用方访问同一个拥有者（single owner）的部署。
//! 独立动态库（dynamic libraries）、多个库版本或多个Wasm实例（Wasm instances）
//! 必须由应用转接到该拥有者，不能各自链接一份后仍假定共享同一时间轴（timeline）。
//! 时间点（time point）只在本拥有者的进程生命周期（process lifetime）内可比较，
//! 不作为Unix时间、跨进程时间或稳定通信编码（stable wire encoding）。
//!
//! 原生合格来源（qualified native sources）可供多线程并发读取和刷新。
//! 浏览器仍限平台来源（platform source）规定的固定JavaScript域（JavaScript realm）
//! 和单线程（single thread）；启用 `browser` 只转接该来源能力，不放宽宿主条件。
//! 休眠累计（suspend accumulation）由 [`try_capabilities`] 描述，不承诺统一行为。
//! WASI宿主陷阱（host traps）、缺失导入（missing imports）与进程中止不转换成普通错误。
//!
//! 本批核心（core）没有内置等待队列（built-in waiting queue）或维护线程
//! （maintenance thread）。外部运行时（runtime）负责其定时队列（timer queue）
//! 的推进及通知；单纯刷新缓存不会主动唤醒外部登记项（external registrations）。
//! 同步和异步刷新（synchronous/async refresh）具有相同发布效果。
//!
//! 以下示例（example）只检查用法能否编译，不在编译检查中初始化全局状态：
//!
//! ```no_run
//! use pi_clock_core::std_time::Duration;
//! use pi_clock_manual::global;
//!
//! # fn example() -> pi_result::Result<()> {
//! global::try_init()?;
//! let now = global::try_now()?;
//! let deadline = now.try_add(Duration::from_millis(10))?;
//! let outcome = global::try_refresh()?;
//! let remaining = deadline.try_duration_since(outcome.published);
//! let _ = remaining; // 到期后逆序差值（reversed difference）会明确报错。
//! # Ok(())
//! # }
//! ```

use std::sync::atomic::AtomicU64;
use std::sync::OnceLock;

use pi_clock_core::source::{MonotonicSource, SourceCapabilities};
use pi_clock_core::std_time::Duration;
use pi_clock_platform::monotonic::PlatformMonotonicSource;
use pi_result::Result;

// 内部设计（internal design），不进入公开文档（API docs）。
// 本实际模块（actual module）保存全局时间轴（global timeline）的全部可变状态。
// GlobalManualState<S>的泛型（generic）只使相同生产算法可在私有局部槽验证，
// 不提供公开多实例（multiple instances）、任意来源注册（source registration）或重置。
// GLOBAL只存成功候选：来源／起点发布后不可变，唯一可变数值为cached_nanos。
// 平台来源的基准（basis）仍由其自己持有；这里不访问其私有基准或原始载荷。
//
// 初始化关键链（initialization chain）：try_init -> init_global_inner
// -> try_install(&GLOBAL, PlatformMonotonicSource::try_prepare)
// -> try_build_state -> 一次try_sample取得起点 -> OnceLock::set。
// try_install先检查已安装，再在set之外调用准备闭包及try_build_state；
// 首样本与AtomicU64::new(0)一起发布。竞争败者释放自己的候选并报AlreadyExists，
// 不像平台来源的重复准备那样复用后返回成功；本次准备失败原样返回，
// 不证明另一次初始化没有成功。源的成功基准不会因上层安装失败而回滚。
// 初始化提交可短暂等待另一提交者；来源调用不在提交同步内执行。
//
// 普通读取链（ordinary-read chain）：try_now -> read_global_inner
// -> GLOBAL.get -> read_state -> read_state_inner -> cached_nanos.load(SeqCst)。
// get观察空槽时直接报InvalidState，不等待正在准备或提交的候选。
// 成功路径（success path）不取系统时间、不分配、不加锁、不重试、不维护队列。
// 所有小委托（small delegations）可内联（inline）；完整公开入口成本另作实测。
//
// 刷新关键链（refresh chain）：try_refresh_async -> try_refresh
// -> refresh_global_inner -> GLOBAL.get -> try_refresh_state -> refresh_state_inner。
// 先try_sample一次，再try_elapsed(&origin, &sample)一次；原来源错误不重分类。
// 取得Duration后以as_nanos的u128结果检查u64::MAX，超界报ResourceExhausted。
// 来源负责对原始差值向下取整；本层不先转换两个端点，也不重新舍入。
// 得到固定candidate后执行cached_nanos.try_update(SeqCst, SeqCst, |old| {
//     if candidate > old { Some(candidate) } else { None }
// })。闭包可能被重试，里面不采样、不换算、不分配、不通知或操作其它状态。
// Ok(previous)表示本次采用：sampled=published=candidate，advanced=true。
// Err(observed)表示成功保留：sampled=candidate，published=observed，advanced=false。
// 这里的Err是标准原子操作（atomic operation）的未更新分支，不是pi_result错误。
// 单值只增不回绕，排除当前值回到更早数值的ABA；重试次数和墙钟耗时没有固定上界。
// 原子读和更新的决定点（decision point）给出本次结果；返回前其它刷新可继续提高缓存。
// 来源失败、计算失败和范围耗尽都发生在本次缓存写入之前，不能撤销他人的成功。
// SeqCst是初始统一内存序（memory ordering），不以读到时钟值替代用户数据同步。
//
// 专用时间点（dedicated time point）只承载同一全局起点下的非负u64纳秒。
// 纯运算（pure arithmetic）不访问GLOBAL，可在安装前计算期限；它不证明读数曾被观察。
// try_from_elapsed把标准Duration显式绑定到这个相对起点，不导入其它时钟身份。
// 显式运算默认无损，溢出、下溢及逆序差返回InvalidInput，不回绕或静默饱和。
// 本批没有std::Instant/SystemTime锚点映射，不能从相同纳秒数推定等价；
// 标准类型的原样入口仍由pi_clock_core::std_time提供。
//
// 依据（references）：quanta 0.13的GLOBAL_RECENT/recent启发单个标量缓存，
// 其set_recent的普通store不满足这里的多写者非递减发布；这里复用Rust1.95
// AtomicU64::try_update条件更新。ArcSwap适合未来不可变多字段配置，当前标量
// 不需要引用计数（reference counting）、借用守卫（borrow guard）和旧快照回收。
// 当前没有对5ns、硬实时（hard real time）或全部平台实际执行作已通过承诺。

// @trace 1
/// 当前全局手推时间轴上的时间点（time point on the current global manual timeline）。
///
/// 表示范围（range）为0至u64::MAX纳秒，约584年；零对应成功安装者保存的首样本。
/// 字段私有，按值复制、比较和散列（hash）不取时或修改时钟。
/// 通过运算构造未来时间点合法，不表示时钟曾经到达过它。
///
/// 只比较同一拥有者、同一进程生命周期的值。跨重启、独立库副本、远端时钟或
/// 模拟分支（simulation branch）不属于这个域；本类型没有反序列化（deserialization）
/// 或任意原始值恢复接口。Debug输出不是稳定编码；不承诺外部二进制接口（FFI）布局。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GlobalManualInstant(u64);

// @trace 2
/// 一次成功刷新所作决定的描述值（description of one successful refresh decision）。
///
/// 相等或迟到样本也成功返回本值；`advanced`区分本次是否实际提高缓存。
/// `published`只对应本次决定点，不保证在函数返回时仍为最新值。
/// 修改或复制这些普通字段不改变时钟，也不构成已经发布某个时间的证明。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RefreshOutcome {
    /// 本次真实样本相对起点的受检时间点（checked sampled time point）。
    pub sampled: GlobalManualInstant,
    /// 本次决定点采用或保留的缓存时间点（adopted or retained cached time point）。
    pub published: GlobalManualInstant,
    /// 本次是否将缓存严格提高（strictly advanced by this operation）。
    pub advanced: bool,
}

// @trace 3
// 固定来源（fixed source）、固定起点（fixed origin）和一个只增的缓存（cache）。
// S无需统一Send/Sync/'static；实际GLOBAL的具体类型必须自然满足静态共享条件，
// 不添加unsafe Send/Sync。局部验证槽不能导出成公开实例或全局替换通路。
struct GlobalManualState<S: MonotonicSource> {
    // 来源（source）按值持有，实际平台句柄共享其永久基准；不持有运行时。
    source: S,
    // 起点样本（origin sample）发布后只借用，不重采样、不因句柄复制改变。
    origin: S::Sample,
    // 仅保存总纳秒（total nanoseconds），首值0；仅try_refresh_state可条件提高。
    cached_nanos: AtomicU64,
}

// @trace 4
// 成功安装槽（successful installation slot）私有，终生不take、替换或重置。
static GLOBAL: OnceLock<GlobalManualState<PlatformMonotonicSource>> = OnceLock::new();

impl GlobalManualInstant {
    /// 全局相对起点（global relative origin）；构造常量不初始化时钟。
    pub const ZERO: Self = Self(0);

    /// 可表示的最大时间点（maximum representable time point），并非当前读数。
    pub const MAX: Self = Self(u64::MAX);

    // @trace 5
    /// 将自全局起点经过的标准时长（elapsed standard duration）显式绑定为时间点。
    ///
    /// `elapsed`必须可无损表示为u64总纳秒；超出范围返回InvalidInput类pi_result。
    /// 不读取或初始化时钟，不认证另一个进程或标准Instant具有同一起点。
    #[inline]
    pub fn try_from_elapsed(elapsed: Duration) -> Result<Self> {
        instant_from_elapsed_inner(elapsed)
    }

    // @trace 6
    /// 无损返回该时间点距全局起点的标准时长（standard duration since the global origin）。
    ///
    /// 不取当前时间、不访问全局实例，不分配或失败；本值不携带Unix日期含义。
    #[inline]
    pub fn duration_since_origin(self) -> Duration {
        instant_duration_inner(self)
    }

    // @trace 7
    /// 增加`duration`并返回新时间点（time point after the supplied duration）。
    ///
    /// 超出u64总纳秒范围返回InvalidInput类pi_result；不回绕或饱和，不推进时钟。
    /// 运算对标准Duration精度无损，成功路径不分配或访问来源。
    #[inline]
    pub fn try_add(self, duration: Duration) -> Result<Self> {
        instant_add_inner(self, duration)
    }

    // @trace 8
    /// 减去`duration`并返回新时间点（time point before the supplied duration）。
    ///
    /// 结果早于零或输入超出可表示范围时返回InvalidInput类pi_result；
    /// 不改变时钟，不以零代替下溢（underflow）。成功路径不分配或访问来源。
    #[inline]
    pub fn try_sub(self, duration: Duration) -> Result<Self> {
        instant_sub_inner(self, duration)
    }

    // @trace 9
    /// 计算`earlier`到本值的非负标准时长（nonnegative standard duration）。
    ///
    /// 同值结果为零；`earlier`较晚时返回InvalidInput类pi_result，不静默取绝对值。
    /// 两值必须属于本类型所述的同一全局域；不读取当前时间或重新取时。
    /// 成功路径（success path）不分配，错误报告（error report）的构造成本另计。
    #[inline]
    pub fn try_duration_since(self, earlier: Self) -> Result<Duration> {
        instant_since_inner(self, earlier)
    }
}

// @trace 10
/// 显式安装进程唯一的全局手推时钟（explicitly install the process-global manual clock）。
///
/// 成功前准备真实单调来源并取得首样本，成功后立即可读零值；后续不自动刷新。
/// 安装成功后再次调用返回AlreadyExists类pi_result，保留原实例及连续时间轴。
/// 发布前来源失败原样返回且可显式重试，不永久保存失败。
///
/// 并发安装允许各自准备，只有一个成功；竞争败者释放候选并报AlreadyExists。
/// 某次准备失败不证明另一调用没有成功，来源已经建立的共享基准不因此回滚。
/// 本函数可能调用系统或JavaScript、分配及短暂等待发布，不启动线程或运行时。
pub fn try_init() -> Result<()> {
    init_global_inner()
}

// @trace 11
/// 读取最近发布的单调时间点（read the most recently published monotonic time point）。
///
/// 未安装或本次尚未观察到成功安装时，返回InvalidState类pi_result；不等待安装，
/// 不隐式初始化。成功值对应本次调用期间成立过的缓存值，可以已过时。
/// 成功路径（success path）不取系统时间、不分配、不加锁、不扫描或通知等待者，
/// 工作量为常数；错误报告（error report）的构造成本另计。
/// 不承诺缓存的新鲜度上界、固定纳秒耗时或任意用户数据的同步效果。
#[inline]
pub fn try_now() -> Result<GlobalManualInstant> {
    read_global_inner()
}

// @trace 12
/// 真实采样并尝试提高全局缓存（sample real time and conditionally advance the global cache）。
///
/// 已安装时只采一次样本、计算相对固定起点的间隔；更大值被采用，相等或迟到值
/// 保留当前缓存。成功返回采样值、决定点的缓存值及是否由本次提高；允许多写者。
/// 未安装返回InvalidState，来源错误原样返回，经过时长超出u64纳秒返回
/// ResourceExhausted；失败只保证本次未写，其他线程仍可成功推进。
///
/// 不分叉、不分配成功结果、不创建任务或扫描队列。来源调用及并发竞争的成本另计，
/// 不保证固定重试次数或墙钟时限。本批不通知外部运行时私有的定时等待者。
pub fn try_refresh() -> Result<RefreshOutcome> {
    refresh_global_inner()
}

// @trace 13
/// 以原生异步函数（native async function）调用同一全局刷新行为。
///
/// 创建返回的异步计算（Future）不执行刷新；首次轮询（first poll）执行与
/// [`try_refresh`]相同的完整同步操作，正常返回Ready，不主动让出执行权或登记唤醒。
/// 未轮询即丢弃无效果；开始执行后不提供中途异步取消点（async cancellation point）。
/// 初始化状态在轮询时检查，创建时尚未初始化不预先决定结果。
/// 无需具体运行时；异步封装（async wrapper）本身不创建任务或额外堆分配。
/// 刷新中的来源调用（source calls）及错误报告（error report）构造成本另计。
/// 原生合格配置可跨线程执行，浏览器继续遵守固定单域限制；错误和发布保证同同步入口。
pub async fn try_refresh_async() -> Result<RefreshOutcome> {
    try_refresh()
}

// @trace 14
/// 查询已安装来源的固定能力（query fixed capabilities of the installed source）。
///
/// 未安装返回InvalidState类pi_result；成功只复制能力描述，不采样、不查询宿主或分配。
/// 返回副本不改变来源；休眠行为未知（unspecified suspend behavior）不能当作排除休眠。
pub fn try_capabilities() -> Result<SourceCapabilities> {
    capabilities_global_inner()
}

// @trace 15
// 内部用途（internal purpose）：在slot安装一次有效手推状态（valid manual state）。
// prepare为一次性来源构造闭包（one-shot source factory），按值持有，只在slot为空时调用。
// slot及S无全局线程约束；生产只传GLOBAL，私有验证可传局部槽，不导出状态句柄。
// 已安装立即AlreadyExists；否则在set之外prepare一次、try_build_state一次，原错误传播。
// set胜出返回Ok(())；败者候选丢弃并报AlreadyExists，不能复用胜者后冒充本次安装成功。
// 不重入调用本slot的安装，不在构造期间等待本slot；公开内置来源不注册任意外部闭包。
fn try_install<S, F>(slot: &OnceLock<GlobalManualState<S>>, prepare: F) -> Result<()>
where
    S: MonotonicSource,
    F: FnOnce() -> Result<S>,
{
    install_inner(slot, prepare)
}

// @trace 16
// 内部用途（internal purpose）：持有source并采首样本，构造未发布的0值候选（zero candidate）。
// source按值转入，try_sample只调用一次；错误原样返回，未发布来源正常丢弃。
// 成功交付source、origin及AtomicU64::new(0)，本函数不读写任何安装槽、不再探测一次。
fn try_build_state<S: MonotonicSource>(source: S) -> Result<GlobalManualState<S>> {
    build_state_inner(source)
}

// @trace 17
// 内部用途（internal purpose）：从有效state加载一个已发布数值（published scalar）。
// 借用仅覆盖本次调用，返回值不借用state；只做一次SeqCst原子加载及专用点包装。
// 不采样、不分配、不重试、不读取其它可变字段，不能把局部槽返回为全局时钟。
#[inline]
fn read_state<S: MonotonicSource>(state: &GlobalManualState<S>) -> GlobalManualInstant {
    read_state_inner(state)
}

// @trace 18
// 内部用途（internal purpose）：按固定起点完成一次采样、受检换算和条件发布（conditional publication）。
// 只借用有效state，不重建来源或起点；成功、失败及相等／迟到的完整链见上方内部设计。
// 闭包（closure）内只比较固定candidate；成功和失败内存序均SeqCst。
// 返回普通描述值，绝不导出AtomicU64、可写借用、锁守卫或全局替换能力。
fn try_refresh_state<S: MonotonicSource>(state: &GlobalManualState<S>) -> Result<RefreshOutcome> {
    refresh_state_inner(state)
}

include!("global_inner.rs");
