//! 准备并复用固定平台单调来源（fixed platform monotonic source）。
//!
//! [`PlatformMonotonicSource::try_prepare`] 成功后可取得不透明样本（opaque samples），
//! 并通过 [`MonotonicSource`] 计算标准时长（standard duration）。来源（source）
//! 不拥有任何时钟的起点、缓存或等待队列（waiting queue），也不启动后台线程。
//! 复制来源句柄（source handle）共享同一基准，不是分叉一个时钟（clock fork）。
//!
//! 构造（construction）、采样（sampling）和间隔计算（elapsed calculation）的函数体（function bodies）已实现。
//! 当前Windows x86_64的公开调用链（public call chain）测试通过；最低支持版本（MSRV）、
//! 其他目标（targets）及最低系统（minimum OS）的验证状态（validation status）分别确认。
//! 以下示例（example）仍仅作编译检查（compile check）。
//!
//! 原生后端（native backends）按目标操作系统选择：Linux／Android使用
//! `CLOCK_MONOTONIC`，Windows使用QPC及准备时保存的QPF频率。前者不累计系统
//! 休眠（system suspend），后者累计；WASI及浏览器的休眠规则由宿主（host）
//! 决定，这里报告 [`pi_clock_core::source::SuspendBehavior::Unspecified`]。
//!
//! WASI预览1（WASI Preview 1）使用原始纳秒读数；预览2（Preview 2）需要宿主
//! 提供 `wasi:clocks/monotonic-clock@0.2.12` 或经验证的兼容映射。
//! 缺失导入（missing imports）发生在实例化（instantiation）阶段；宿主陷阱
//! （host traps）与进程中止不被转换成 pi_result 错误。
//!
//! 浏览器后端（browser backend）需显式启用 `browser`，仅支持可信且合格的固定
//! JavaScript域（JavaScript realm）、单线程（single thread）、无共享内存
//! （unshared memory）的 wasm32 配置。启用原子指令（atomics）的配置返回
//! Unsupported；编译配置（build configuration）本身不认证全部宿主装配条件。
//! 初次准备保存实际 `Performance` 对象及 `now` 函数；后续不重新绑定它们。
//! 属性读取及调用中的同步异常（synchronous exceptions）经捕获后返回错误，
//! 不改用系统日期（wall-clock time）或伪造零值。成功读数仍受浏览器降精度
//! （precision coarsening）及浮点舍入（floating-point rounding）影响。
//!
//! 本批支持配置（support profile）为Windows x86_64的Windows10／Server2016以上；
//! GNU Linux x86_64的内核（kernel）3.2及glibc2.17以上、aarch64的内核4.1及
//! glibc2.17以上；Android aarch64的API21以上；以及上述Wasm／WASI配置。
//! 其他目标尚未验证；没有合格后端时准备返回Unsupported。准备成功也不证明
//! 硬件准确度（hardware accuracy）、纳秒分辨率（nanosecond resolution）或固定耗时。
//!
//! ```no_run
//! use pi_clock_core::source::MonotonicSource;
//! use pi_clock_platform::monotonic::PlatformMonotonicSource;
//!
//! # fn example() -> pi_result::Result<()> {
//! let source = PlatformMonotonicSource::try_prepare()?;
//! let shared_source = source;
//! let earlier = source.try_sample()?;
//! let later = shared_source.try_sample()?;
//! let elapsed = source.try_elapsed(&earlier, &later)?;
//! let _ = elapsed;
//! # Ok(())
//! # }
//! ```

#[cfg(target_os = "windows")]
use std::num::NonZeroU64;
use std::sync::OnceLock;

use pi_clock_core::source::{MonotonicSource, SourceCapabilities};
use pi_clock_core::std_time::Duration;
use pi_result::Result;

// 一个公开来源家族（source family）在合法部署范围内只发布一个基准。
// 所有公开样本只能由已准备句柄的采样出口产生；私有载荷不能由安全Rust外部构造，
// 不提供反序列化、裸数值构造或外部二进制接口（FFI）恢复通路。
// 因而同一家族的样本相容性由构造出口和永久固定基准共同建立，不逐样本分配身份，
// 不用对象地址证明同域；独立库副本、Wasm实例或重启后的裸计数不在此相容承诺内。

// @trace 1
/// 已准备的平台单调来源句柄（prepared platform monotonic-source handle）。
///
/// 字段私有；只能显式准备后得到。Clone／Copy复制共享借用（shared borrow），
/// 不重新取时、不克隆引用计数、不更换基准，也不复制任何上层时钟的状态。
/// 丢弃本句柄不销毁基准，不使旧样本失效；基准保留到当前部署作用域结束。
///
/// 原生支持配置内允许跨线程并发使用；浏览器仍须满足本模块的单域／单线程条件，
/// 类型自动具备的特征（auto traits）不扩大宿主运行域（host operating domain）。
/// 没有Default、关闭、重置、重新绑定或任意原始值构造接口。
#[derive(Clone, Copy)]
pub struct PlatformMonotonicSource {
    basis: &'static PreparedBasis,
}

// @trace 2
/// 该来源家族产生的不透明原始样本（opaque raw sample）。
///
/// 可按值保存和复制；复制不重新采样、不分配身份或克隆共享句柄。
/// 通过来源的try_elapsed借用计算间隔；没有公开字段、数值转换、排序或稳定编码。
/// Debug仅用于诊断（diagnostics），不构成通信格式（wire format）。
///
/// 样本不随来源句柄丢弃而失效，但不能跨机器、系统重启、独立拥有者或Wasm实例
/// 交换后仍推定同域；它也不是Unix时间点或上层时钟的相对时间点。
#[derive(Debug, Clone, Copy)]
pub struct PlatformMonotonicSample {
    raw: RawSample,
}

// @trace 3
// 基准元数据（basis metadata）由BASIS唯一发布，发布后不可变。
// capabilities保存准备时确定的描述值；复制其返回值不改变本对象。
// Windows frequency是正的每秒计数，不能用整数纳秒因子代替。
// 浏览器performance／now保留准备时选定的对象和函数，避免后续重读全局属性换源。
// JavaScript句柄的Send／Sync依赖上游条件实现；仅在下列非atomics配置纳入字段，
// 不自行添加unsafe Send／Sync实现，也不据此允许跨worker部署。
struct PreparedBasis {
    capabilities: SourceCapabilities,
    #[cfg(target_os = "windows")]
    frequency: NonZeroU64,
    #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics")))]
    performance: wasm_bindgen::JsValue,
    #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics")))]
    now: js_sys::Function,
}

// @trace 4
// 按编译目标（compile target）保留原始载荷；浏览器先分别量化端点会改变间隔语义，
// 因而保留f64毫秒。Unix纳秒字段先在采样出口验证0..1_000_000_000后存为u32。
// 不支持配置没有变体，不能安全构造样本；实际准备入口返回Unsupported。
#[derive(Debug, Clone, Copy)]
enum RawSample {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    Unix { seconds: i64, nanoseconds: u32 },
    #[cfg(target_os = "windows")]
    Windows(i64),
    #[cfg(all(target_os = "wasi", target_env = "p1"))]
    WasiPreview1(u64),
    #[cfg(all(target_os = "wasi", target_env = "p2"))]
    WasiPreview2(u64),
    #[cfg(all(target_arch = "wasm32", target_os = "unknown", feature = "browser", not(target_feature = "atomics")))]
    Browser(f64),
}

// @trace 5
// 只保存成功基准，不保存Result；本库不提供take／替换／原始可写访问。
// 原生作用域为一个确定的拥有者，浏览器为上述固定realm／Wasm拥有者；
// 多个独立库副本或Wasm实例必须按应用的单一拥有者约定接入，不能自动合并。
static BASIS: OnceLock<PreparedBasis> = OnceLock::new();

impl PlatformMonotonicSource {
    // @trace 6
    /// 显式准备或复用当前基准（prepare or reuse the current basis）。
    ///
    /// 已就绪时直接返回共享句柄，不再探测或采样。首次尝试在发布之外准备元数据并
    /// 探测一次真实读数，探测值不向外发布，也不作为任何上层时钟的起点。
    /// 准备／探测失败返回pi_result，失败结果不被永久保存，后续可显式重试。
    ///
    /// 并发首次尝试允许各自准备；只有一个候选发布，其余成功者丢弃本地候选并复用
    /// 已发布基准。某次尝试失败不撤销另一调用的成功；Err不证明全局基准仍为空。
    /// 没有把可失败操作放进OnceLock构造闭包，不依赖未稳定的get_or_try_init。
    ///
    /// 不支持目标／缺少可用浏览器能力返回Unsupported；系统或绑定调用失败、
    /// 返回无效原始数据为Dependency并保留原因。宿主陷阱／中止另守模块边界。
    /// 准备可能调用系统／JavaScript并分配，提交可能短暂等待另一个发布者；
    /// 不持发布内部同步跨越准备／探测，不创建线程、时钟缓存或运行时。
    pub fn try_prepare() -> Result<Self> {
        source_prepare_inner()
    }
}

// @trace 7
impl MonotonicSource for PlatformMonotonicSource {
    type Sample = PlatformMonotonicSample;

    // @trace 8
    /// 采集本次调用期间成立的真实单调观测（real monotonic observation）。
    ///
    /// 不重新准备或换源；只使用当前固定基准。普通失败返回Dependency类pi_result，
    /// 不返回伪造零值；宿主陷阱和中止不在可恢复范围内。成功零计数本身合法。
    /// 不分配样本身份、不克隆共享句柄、不扫描队列；系统、绑定及错误包装成本另计，
    /// 不承诺固定纳秒耗时或整个依赖路径等待自由（wait-free）。
    fn try_sample(&self) -> Result<Self::Sample> {
        source_sample_inner(self)
    }

    // @trace 9
    /// 计算本来源家族两个样本的受检非负间隔（checked nonnegative elapsed duration）。
    ///
    /// 只借用参数和固定基准，不重新取时或改变任何状态；同一有效样本的成功值为零。
    /// 逆序／载荷不相容／结果超出标准Duration范围返回InvalidInput类pi_result；
    /// 不静默归零、饱和或回绕。正常域内没有关闭／重绑导致的样本失效。
    ///
    /// Unix及WASI保持整数差；Windows用受检宽整数先计算原始计数差，再乘十亿、
    /// 除固定频率后向下取整。浏览器先对原始f64毫秒作差，再换算并向下取整纳秒，
    /// 公开保留IEEE浮点误差，不承诺实数意义的精确向下取整；不先量化两个端点。
    /// 标准Duration范围不被提前缩窄为u64总纳秒，后者由具体时钟消费者另行检查。
    fn try_elapsed(
        &self,
        earlier: &Self::Sample,
        later: &Self::Sample,
    ) -> Result<Duration> {
        source_elapsed_inner(self, earlier, later)
    }

    // @trace 10
    /// 返回准备后固定的能力描述（fixed source capabilities）。
    ///
    /// 只复制描述值，不取时、不查询宿主、不分配或启动维护；不认证准确度及样本身份。
    fn capabilities(&self) -> SourceCapabilities {
        source_capabilities_inner(self)
    }
}

// @trace 11
// 内部用途（internal purpose）：复用或发布一次成功准备的基准（prepared basis）。
// 本函数（function）只供当前模块（module）内部使用，不是公开接口（public API）。
// 参数（parameters）：slot 是一次写入槽（one-time initialization slot）的共享借用（shared borrow）；
// build 是持有所有权（ownership）的构造闭包（construction closure），probe 是持有所有权（ownership）
// 的探测闭包（probe closure）；两者均采用静态分发（static dispatch），不是运行时注册（runtime registration）。
// 返回（return）：成功时借用已发布基准（published basis），生命周期（lifetime）受 slot 的借用约束；
// 生产入口（production entry）只传 BASIS，局部协议验证（local protocol verification）可传局部槽（local slot），
// 后者不产生公开来源句柄（public source handle），也不增加生产基准（production basis）。
// 错误（errors）：构造（construction）或探测（probe）失败时原样返回 pi_result 错误并释放本地候选，
// 本次不写 slot；错误返回不证明并发调用（concurrent call）没有成功发布（publication）。
// 副作用（side effects）：已就绪时不执行 build 或 probe；未就绪时在发布同步（publication synchronization）
// 之外依次构造（construction）和探测（probe），各至多一次；通过后尝试一次提交（commit）。
// 竞争失败时释放候选并复用胜者；不清空或替换 slot，未调用的闭包（closure）仍正常释放。
// 成本（cost）：已就绪时只检查并借用；首次成本包含闭包（closure）的工作及可能短暂等待的提交（commit），
// 不在发布同步（publication synchronization）内执行系统调用（system call）或 JavaScript，
// 不创建线程（thread）、异步运行时（async runtime）或自旋重试（spin retry）。
fn prepare_once<'a, F, P>(
    slot: &'a OnceLock<PreparedBasis>,
    build: F,
    probe: P,
) -> Result<&'a PreparedBasis>
where
    F: FnOnce() -> Result<PreparedBasis>,
    P: FnOnce(&PreparedBasis) -> Result<()>,
{
    prepare_once_inner(slot, build, probe)
}

// @trace 12
// 内部用途（internal purpose）：只准备当前编译目标（compile target）的基准元数据（basis metadata）。
// 本函数（function）只供当前模块（module）内部使用，不是公开接口（public API）。
// 参数（parameters）：无；目标平台（target platform）与功能（feature）由当前编译配置（build configuration）确定。
// 返回（return）：成功时交付尚未发布的基准（unpublished basis），其能力描述（capability description）及
// 平台数据（platform data）供后续探测（probe）、发布（publication）、采样（sampling）与间隔计算（elapsed calculation）使用。
// 错误（errors）：不支持的目标、缺失浏览器能力（browser capability），或浏览器对象（browser object）
// 与函数（function）不符合预期时返回 Unsupported；
// 系统调用（system call）、绑定调用（binding call）或同步异常（synchronous exception）失败返回 Dependency，保留原因；
// 宿主陷阱（host trap）、缺失导入（missing import）及进程中止（process termination）仍按本模块（module）边界处理。
// 副作用（side effects）：可查询系统能力（system capability）或 JavaScript 属性并持有准备结果，
// 不发布基准（basis）、不建立时钟起点（clock origin）、不创建缓存（cache）或后台工作（background work）。
// 成本（cost）：取决于对应系统调用（system call）、绑定调用（binding call）与错误构造（error construction），
// 可能发生分配（allocation）；不承诺固定耗时（fixed latency）。
fn prepare_basis() -> Result<PreparedBasis> {
    prepare_basis_inner()
}

// @trace 13
// 内部用途（internal purpose）：借用既定基准（prepared basis）执行一次真实原始采样（raw real sampling）。
// 本函数（function）只供当前模块（module）内部使用，不是公开接口（public API）。
// 参数（parameters）：basis 是已准备基准（prepared basis）的共享借用（shared borrow）；
// 准备探测（preparation probe）可传尚未发布的候选基准（candidate basis），正常采样（sampling）使用已发布基准（published basis）。
// 返回（return）：成功时交付该基准（basis）对应的原始载荷（raw payload）；真实零值（real zero value）合法，
// 本函数（function）不包装公开样本（public sample）。
// 错误（errors）：系统调用（system call）、绑定调用（binding call）失败或无效返回值（invalid return value）
// 返回 Dependency 并保留原因，不伪造零值；宿主陷阱（host trap）与进程中止（process termination）不转成错误返回。
// 副作用（side effects）：读取一次系统或宿主来源（host source），不修改 basis、不重新绑定来源（source rebinding），
// 不逐次分配样本身份（sample identity）、克隆共享句柄（shared handle）或取得管理锁（management lock）。
// 成本（cost）：包含对应系统调用（system call）、绑定调用（binding call）及可能的错误分配（error allocation），
// 不承诺固定耗时（fixed latency）或整个依赖路径（dependency path）的等待自由（wait freedom）。
fn sample_raw(basis: &PreparedBasis) -> Result<RawSample> {
    sample_raw_inner(basis)
}

// @trace 14
// 内部用途（internal purpose）：按固定基准（fixed basis）计算两个原始载荷（raw payloads）的受检直接差（checked direct difference）。
// 本函数（function）只供当前模块（module）内部使用，不是公开接口（public API）。
// 参数（parameters）：basis 是已准备基准（prepared basis）的共享借用（shared borrow）；
// earlier 与 later 分别借用起始原始载荷（start raw payload）和终止原始载荷（end raw payload），不消费它们。
// 返回（return）：成功时交付向下取整（round down）到整数纳秒（integer nanoseconds）的标准时长（standard duration），
// 同一有效值（valid value）的差为零；浏览器的浮点舍入（floating-point rounding）仍遵守公开说明。
// 错误（errors）：载荷不相容（incompatible payloads）、逆序（reverse order）、无效数值（invalid numeric value）
// 或超过标准时长范围（standard-duration range）时返回 InvalidInput，不静默归零、饱和（saturation）或回绕（wraparound）。
// 副作用（side effects）：不采样（sampling）、不修改参数（parameters）、基准（basis）或能力描述（capability description），
// 不查询宿主（host）、不建立可写世代（mutable generation）或撤销注册表（revocation registry）。
// 成本（cost）：只执行局部受检算术（local checked arithmetic）及必要的错误构造（error construction），
// 错误路径（error path）可能分配（allocation）；不引入系统调用（system call）或同步等待（synchronization wait）。
fn elapsed_raw(
    basis: &PreparedBasis,
    earlier: &RawSample,
    later: &RawSample,
) -> Result<Duration> {
    elapsed_raw_inner(basis, earlier, later)
}

include!("monotonic_inner.rs");
