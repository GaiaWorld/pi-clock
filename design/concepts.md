当前平台来源（platform source）的五个真实函数体（function bodies）已实现；Windows公开调用链（public call chain）已有真实测试。以下旧占位及旧检查表述只保留其当时范围，本次仍未实现的是新手推候选（manual-clock candidate）的13个业务占位。

# 基础时间概念（foundational time concepts）

本页解释基础来源声明（source declarations）、标准时间兼容入口（standard-library time compatibility entry）与平台单调来源（platform monotonic source）使用的概念。

| 概念（concept） | 含义与关系（meaning and relationship） |
| --- | --- |
| 单调来源（monotonic source） | MonotonicSource定义采样、受检间隔和固定能力查询。具体提供者（provider）负责真实观测及运行条件。 |
| 不透明样本（opaque sample） | Sample由具体来源定义表示；采样按值返回，消费者（consumer）可移动保存起点，计算时共享借用。相同类型或数值不证明计量基准相容。 |
| 计量基准（measurement basis） | 来源解释样本及判断比较相容性的依据；有效使用期保持，系统日期跳变不改变它。不同对象可共享合法基准，也可能不相容。 |
| 来源间隔（source elapsed duration） | 从earlier到later的原始直接经过量，受检后向下取整为标准Duration。逆序、不能确认相容、超界或失效不得伪造成功；取整后区间不保证可加。 |
| 来源能力（source capabilities） | SourceCapabilities是描述值（description value），当前只含suspend。复制或修改副本不改变来源，不证明准确度或样本身份。 |
| 休眠累计行为（suspend behavior） | SuspendBehavior的Includes／Excludes／Unspecified描述系统休眠经过量是否计入；与线程休眠或唤醒准确度不同。 |
| 标准时间兼容入口（standard-library time compatibility entry） | std_time以三个独立公开重导出（public re-exports）提供标准Duration、Instant和SystemTime名称；不增加包装、转换或时钟状态。 |
| 类型身份（type identity） | 重导出名与原std::time项指向同一个Rust类型，值可直接交给要求原类型的接口（API），无需From／Into或表示换算。 |
| 标准时长（standard duration） | std::time::Duration表示非负时长；source返回的Duration与std_time::Duration相同，不因重导出自动缩小为u64纳秒范围。 |
| 标准单调时间点（standard monotonic instant） | std::time::Instant沿用标准库的不透明表示及宿主语义；其now方法不读取本库手推缓存。 |
| 系统日期时间点（system-time instant） | std::time::SystemTime沿用系统日期来源，可能受系统校时影响；重导出不使它成为本库独立校正或手推时间点。 |
| 起点映射（origin mapping） | 手推或自定义时间点与标准时间点等价转换所需的关系，须有充分起点、同域、范围及精度上下文；裸纳秒数不足以建立该关系，当前未实现。 |
| 平台来源句柄（platform source handle） | PlatformMonotonicSource以私有的静态共享借用指向PreparedBasis。Clone／Copy共享固定基准，不重新准备、取时或克隆引用计数，也不是分叉上层时钟。 |
| 平台原始样本（platform raw sample） | PlatformMonotonicSample以私有RawSample保存目标平台的原始读数。可复制但不重新采样；没有公开原始值构造、排序、序列化或稳定编码。 |
| 已准备基准（prepared basis） | PreparedBasis保存固定能力描述及目标所需元数据，由BASIS的一次写入单元（OnceLock）保存成功值；没有公开替换或清空入口。 |
| 来源准备与探测（source preparation and probing） | try_prepare的合同要求先准备元数据并探测一次真实读数，再发布基准。探测不产生公开样本或任何上层起点；失败可显式重试，某次失败不证明其他并发尝试未成功。 |
| 原始载荷（raw payload） | RawSample按编译目标保留原始计数表示，间隔计算先求直接差值再换算。浏览器保留浮点毫秒，不能把各端点先量化为整数纳秒。 |
| 部署作用域（deployment scope） | 固定基准的相容承诺限于一个确定拥有者及其合法宿主范围；浏览器还限于固定JavaScript域（JavaScript realm）。独立库副本、Wasm实例、机器或重启后的计数不能直接推定同域。 |

历史状态（historical status；以下仅保留原占位阶段，当前平台来源五个函数体均已实现）：基础来源定义见[source.rs](../crates/core/src/source.rs)，标准兼容名称见[std_time.rs](../crates/core/src/std_time.rs)，平台来源及样本声明见[monotonic.rs](../crates/platform/src/monotonic.rs)。平台来源的五个私有实现入口仍为未实现占位（unimplemented stubs），上述准备与取时合同尚未成为可运行能力。


## 全局手推核心补充（global manual-core additions）

全局手推时间点（global manual time point）是同一拥有者、同一进程生命周期内自固定起点经过的u64整数纳秒；它不是标准Instant或Unix日期。受检运算（checked arithmetic）允许构造未来期限，不认证时钟曾到达该值。

刷新结果（refresh outcome）描述一次采样在其原子决定点被采用，或保留已有缓存（包括相等与迟到样本）；它不是返回时绝对最新状态，也不是不可伪造的发布证明。来源（source）只提供不透明样本、受检间隔与固定能力；手推状态（manual state）独立保存自己的起点及缓存。

当前API命名和文件文字是待用户审阅的候选（candidate），业务函数尚未实现。


## 当前全局手推声明位置（current global manual declarations）

GlobalManualInstant及RefreshOutcome当前实际声明于crates/manual/src/global.rs；全局起点和样本／决定点的含义保持前述定义。前述待审候选表述属于原候选阶段；当前声明以crates/manual/src/global.rs为准。所有点值均在同一拥有者／进程生命周期的全局域内，普通字段副本不授予修改缓存的权限。
