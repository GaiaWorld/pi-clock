//! 真实单调来源（monotonic source）的采样与间隔契约（sampling and elapsed-duration contract）。
//!
//! 使用具体来源（concrete source）取得起点样本（origin sample），随后借用两份同源
//! 样本（same-source samples）计算经过时长（elapsed duration）。本模块（module）
//! 不持有全局时钟（global clock），也不创建后台线程（background thread）。

use std::time::Duration;

use pi_result::Result;

// @trace 1
/// 来源对系统休眠（system suspend）的累计行为（accumulation behavior）。
///
/// 描述所查询来源（source）的规则；不表示线程休眠（thread sleep）、应用暂停
/// （application pause）或唤醒准确度（wake-up accuracy）。未知行为不能当作排除休眠。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SuspendBehavior {
    /// 系统休眠（system suspend）期间的经过时间计入来源读数（source reading）。
    Includes,
    /// 系统休眠（system suspend）期间的经过时间不计入来源读数（source reading）。
    Excludes,
    /// 没有足够依据承诺统一行为，或行为取决于具体宿主（host）。
    Unspecified,
}

// @trace 2
/// 该来源（source）的最低层能力描述（foundational capability description）。
///
/// 本值是可复制的描述值（description value），修改副本不改变来源配置
/// （source configuration）。它不携带来源身份（source identity）、准确度
/// （accuracy）证明、当前读数（current reading）或可写内部状态（mutable state）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceCapabilities {
    /// 来源对系统休眠（system suspend）的累计行为（accumulation behavior）。
    pub suspend: SuspendBehavior,
}

// @trace 3
// 普通关联类型（associated type）保留来源自己的样本表示（sample representation）；
// 借用参数（borrowed parameter）避免要求调用者复制或克隆起点，不保证比传小整数更快。
// 能力描述值（capability description value）与样本身份（sample identity）分开，
// 具体提供者（provider）须验证自己的计量域（measurement domain），不能凭类型名推定。
/// 采集不透明样本（opaque sample）、计算同源间隔（same-source elapsed duration）
/// 并查询固定来源能力（source capabilities）的一组内聚行为（cohesive behavior）。
///
/// 来源的计量基准（measurement basis）在该来源的有效使用期间保持不变；系统日期
/// 时间调整（wall-clock adjustment）不能令其跳变。允许底层操作系统（OS）的频率
/// 校正（frequency correction）。具体后端（backend）负责真实取时（real sampling）
/// 与其运行条件，单凭实现本特征（trait）不证明物理准确度（physical accuracy）。
///
/// 本特征（trait）不统一要求来源或样本实现 Send、Sync、Copy、Clone 或 'static。
/// 跨线程（cross-thread）或全局消费者（global consumer）须对实际保存与共享的类型
/// （type）另加所需约束（bounds）。成功和可预期失败由返回值表达，不用恐慌（panic）
/// 代替普通来源错误（source error）；宿主陷阱（host trap）和进程中止不被转换为错误返回。
///
/// 三个同步方法（synchronous methods）不要求运行时（runtime）。声明本身没有默认
/// 方法或持有状态；具体提供者须说明阻塞（blocking）、分配（allocation）、回调
/// （callback）、同步（synchronization）、资源副作用及算法复杂度（algorithmic complexity）。
/// 能否在异步任务（async task）内直接调用，须按这些成本判断；本接口（API）没有统一
/// 时限、异步取消（async cancellation）或外部二进制接口（FFI）布局保证。
///
/// 以下用法只组合来源操作（source operations），具体来源的准备条件须先满足：
///
/// ```
/// use std::time::Duration;
/// use pi_clock_core::source::MonotonicSource;
/// use pi_result::Result;
///
/// fn measure<S: MonotonicSource>(source: &S) -> Result<Duration> {
///     let earlier = source.try_sample()?;
///     let later = source.try_sample()?;
///     source.try_elapsed(&earlier, &later)
/// }
/// ```
pub trait MonotonicSource {
    /// 由具体实现（implementation）定义表示的样本（sample）。
    ///
    /// 采样结果按值交付（owned return），可移动到消费者（consumer）保存；后续计算
    /// 借用它，不要求复制。普通关联类型（associated type）不能随本次 &self 的借用
    /// 生命周期（borrow lifetime）变化；可持有独立资源或来源类型已有的外部借用，
    /// 不从本次调用泄露临时内部缓冲区（temporary internal buffer）的借用。
    type Sample;

    // @trace 4
    /// 读取来源在本次调用期间成立的单调观测（monotonic observation）。
    ///
    /// 成功返回由调用者持有的样本（sample）；它不自动发布到全局缓存（global cache）。
    /// 可预期来源失败返回 pi_result::Result 的错误；不能暗中用系统日期
    /// （wall-clock time）、模拟增量（simulated increment）或伪造零值代替失败。
    /// 具体来源（source）须说明采样成本（sampling cost）和宿主限制（host limitation）；
    /// 本声明（declaration）不保证全部实现无锁（lock-free）或固定时限完成。
    fn try_sample(&self) -> Result<Self::Sample>;

    // @trace 5
    /// 计算 earlier 到 later 的非负同源间隔（nonnegative same-source elapsed duration）。
    ///
    /// earlier为起始样本（start sample），later为终止样本（end sample）；调用者
    /// 保留两者的所有权（ownership），不要求复制或克隆。允许借用同一个有效样本
    /// （valid sample），此时成功值为 Duration::ZERO，验证或来源依赖失败仍返回错误。
    /// 来源须确认两个样本在其已声明的计量基准（measurement basis）下相容；不同
    /// 来源对象或地址本身不决定相容性。无法确认相容、顺序相反或结果超出 Duration
    /// 范围时，返回 InvalidInput 分类的 pi_result 错误；
    /// 已失效样本（expired sample）返回 InvalidState，不静默饱和（saturate）或回绕（wrap）。
    /// 多项拒绝条件（rejection conditions）同时成立时，可返回任一已确认条件对应的
    /// 分类，并说明实际原因；不承诺校验顺序（validation order）。
    ///
    /// 来源计数（source counter）需要换算时，对这两个原始样本的直接差值
    /// （direct difference）向下取整（round down）到整数纳秒；不能先各自取整再相减。
    /// 来源分辨率（source resolution）和浮点误差（floating-point error）仍由具体
    /// 来源说明。取整后的间隔不保证区间可加性（interval additivity）。
    ///
    /// 不重新采样，不消费或修改参数，不改变计量基准（measurement basis）或能力
    /// 描述（capability description）。具体实现（implementation）须声明必要同步
    /// （synchronization）及成本，不能仅由共享借用（shared borrow）推定纯度或线程安全。
    fn try_elapsed(
        &self,
        earlier: &Self::Sample,
        later: &Self::Sample,
    ) -> Result<Duration>;

    // @trace 6
    /// 返回该来源有效使用期间固定的能力描述（source capabilities）。
    ///
    /// 普通查询（query）不采样，不启动维护工作（maintenance work），不改变来源；
    /// 来源准备（source preparation）时无法可靠辨认的休眠行为（suspend behavior）
    /// 使用 Unspecified。结果（result）不证明两个样本具有同源身份（source identity）。
    fn capabilities(&self) -> SourceCapabilities;
}
