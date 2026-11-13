当前平台来源（platform source）的五个真实函数体（function bodies）已实现；Windows公开调用链（public call chain）已有真实测试。以下旧占位及旧检查表述只保留其当时范围，本次仍未实现的是新手推候选（manual-clock candidate）的13个业务占位。

# 基础时间声明（foundational time declarations）

历史状态（historical status；以下仅保留原占位阶段，当前平台来源五个函数体均已实现）：源码根（source roots）：crates/core/src与crates/platform/src。基础库（core crate）的公开入口（public entry）为lib.rs，来源契约（source contract）为source.rs，标准兼容入口（standard compatibility entry）为std_time.rs；平台库（platform crate）的公开入口为lib.rs，来源声明及连接为monotonic.rs，私有实现入口（private implementation entries）为monotonic_inner.rs，后者由monotonic.rs末尾的include!("monotonic_inner.rs")纳入同一个Rust模块（module）。其中五个私有实现函数（private implementation functions）仍为todo!未实现占位（unimplemented stubs），真实取时尚未实现。

## 平台库实际验证（platform-crate verification）

旧组织（previous organization）使用implementation子模块（submodule）与monotonic_impl.rs。以下历史检查（historical checks）已在本项目根目录（project root）完成，平台为Linux x86_64，工具链（toolchain）为rustc 1.99.0-nightly；使用`CARGO_HOME=/tmp/pi-clock-u0001-delivery/cargo-home`及`CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse`。

- `cargo build -p pi-clock-platform`：返回0，完整平台库构建（crate build）通过；有12条未使用项／死代码警告（unused/dead-code warnings），五个todo!占位仍保留。
- `cargo test -p pi-clock-platform --doc --locked --offline`：返回0，1项no_run示例编译通过、0项失败；没有执行该示例的取时路径。
- `cargo doc -p pi-clock-platform --no-deps --locked --offline`：返回0，公开文档生成（documentation generation）通过。

当前monotonic.rs通过include!("monotonic_inner.rs")纳入同一模块（module）的组织，已在本项目根目录（project root）、Linux x86_64上取得以下实际结果；普通构建（build）与文档命令（documentation commands）使用上述CARGO_HOME及稀疏注册表配置（sparse registry configuration）。

- `cargo build -p pi-clock-platform --locked --offline`：退出0，完整平台库构建（crate build）通过；保留13条与未实现占位（unimplemented stubs）相关的未使用项／死代码警告（unused/dead-code warnings）。
- `cargo test -p pi-clock-platform --doc --locked --offline`：退出0，1项no_run示例编译（example compilation）通过、0项失败；未执行取时路径（sampling path）。
- `cargo doc -p pi-clock-platform --no-deps --locked --offline`：退出0，公开文档生成（documentation generation）通过；两种公开不透明类型（public opaque types）、四个方法（methods）、私有项不导出（private items not exported）及未实现提示（unimplemented notice）的页面核对（page checks）通过。
- `cargo check -p pi-clock-platform --locked --offline`：通过Dylint 6.3.2驱动接口（driver API）加载固定修补源码（fixed patched source）生成的检查库（lint library），退出0。此次结果覆盖Linux驱动接口（driver API）检查；未执行cargo-dylint命令行（CLI），也未重新执行Windows新路径的动态库加载（dynamic-library loading）。

历史状态（historical status；以下仅保留原占位阶段，当前平台来源五个函数体均已实现）：上述历史结果与本轮结果分别覆盖旧组织（previous organization）和当前组织（current organization），不能证明最低支持Rust版本（MSRV）1.95、其他目标的运行、真实取时或性能。五个私有实现函数（private implementation functions）仍为todo!未实现占位（unimplemented stubs），尚未验证运行取时（runtime sampling）；手推时钟与自动维护也未实现。基础库已有检查结果保留如下，单独限定其适用范围。

## 既有基础库验证（previous core-crate verification）

既有来源声明此前的构建（build）和1项公开文档测试（doctest）已通过；以下为加入std_time后的实际外部检查结果，不将更早的结果当成新增模块的证明。

上述基础库检查的执行目录（working directory）为项目根目录`/mnt/e/rust/pi_mastser/crate master/tools/pi-clock`，工具链（toolchain）为rustc 1.99.0-nightly，平台为Linux x86_64；环境变量（environment variables）为`CARGO_HOME=/tmp/pi-clock-u0001-delivery/cargo-home`及`CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse`。

- `cargo build -p pi-clock-core --locked --offline`：返回0，库构建（crate build）通过。
- `cargo test -p pi-clock-core --doc --locked --offline`：返回0，2项通过、0项失败；覆盖既有source用例和新增std_time类型身份（type identity）用例。
- `cargo doc -p pi-clock-core --no-deps --locked --offline`：返回0，公开文档生成（documentation generation）通过。

历史状态（historical status；以下仅保留原占位阶段，当前平台来源五个函数体均已实现）：上述基础库结果只覆盖当时的pi-clock-core声明，不作为平台库的证明；最低支持Rust版本（MSRV）1.95或其他平台未验证。真实来源（real source）、手推时钟（manually refreshed clock）和自动维护（automatic maintenance）仍未实现，不将构建或类型用例通过解释为这些能力已可运行。

## 基础来源契约（foundational source contract）

基础契约为已经决定的Rust 2021声明：一个MonotonicSource特征（trait）、SourceCapabilities结构体（struct）与SuspendBehavior枚举（enum）。具体签名（signatures）、字段（field）、变体（variants）及完整使用条件以[source.rs](../crates/core/src/source.rs)为准，公开出口由[lib.rs](../crates/core/src/lib.rs)提供。

具体平台计数与宿主条件不同，来源提供者（source provider）保留自己的Sample表示，并提供采样、同域受检间隔（checked elapsed duration）及固定能力查询。采样返回拥有值，计算共享借用起点与终点，不统一强制Copy／Clone／Send／Sync或静态生命周期；实际全局或跨线程消费者（cross-thread consumer）另加所需约束（bounds）。普通关联类型（associated type）不随本次接收者借用变化。

该选择保留不能复制或持有资源的样本（sample），代价是提供者须说明并验证计量基准（measurement basis）。统一Copy样本按值传递（pass by value）也适用于小型计数，但会限制通用来源范围；尚无数据证明两者存在普遍性能优劣。具体提供者仍可采用Copy样本。

计算成功须确认样本有效、相容、有序且结果可表示；原始直接差值向下取整到整数纳秒。有效同一样本的成功值为零，来源依赖仍可失败。无法确认相容、逆序及范围非法用pi_result的InvalidInput，失效用InvalidState；多项拒绝条件同时成立时可返回任一已确认原因对应的分类并说明原因，不规定检查顺序。其他来源依赖失败由实现说明，普通失败不能用恐慌（panic）、回绕或静默饱和代替。

能力查询返回固定描述值；未知休眠规则用Unspecified。修改描述副本不改变来源。来源声明不持有全局对象（global object）、缓存（cache）、等待队列（wait queue）或维护线程（maintenance thread），没有默认方法或未实现占位（unimplemented stub）。具体提供者负责阻塞、分配、同步、回调、副作用和实际复杂度的说明，声明不自动证明其语义或物理准确度。

历史状态（historical status；以下仅保留原占位阶段，当前平台来源五个函数体均已实现）：具体来源的真实取时、手推缓存（manual-clock cache）、上层时钟的全局安装（global clock initialization）及自动推进（automatic refresh）仍未实现。平台来源当前的声明与未实现入口见后文，不能当作这些能力的运行验证结果。

## 标准时间兼容入口（standard-library time compatibility entry）

[std_time.rs](../crates/core/src/std_time.rs)以三个独立公开重导出（public re-exports）绑定std::time::Duration、std::time::Instant和std::time::SystemTime，分别使用#[doc(no_inline)]，将标准类型说明保留在原文档入口。[lib.rs](../crates/core/src/lib.rs)公开std_time模块，原source入口保留；source.rs仍直接引用std::time::Duration，没有source与std_time之间的新依赖。

这些名称保持标准库类型身份（type identity），没有包装类型（newtype wrapper）、表示换算或额外分配。已有标准值可以直接传入要求对应原类型的接口（API），反向传值也一样；这不构成手推或自定义时间点的转换算法。Duration仍为标准非负时长，不自动缩小为手推内部计数的范围；Instant保持不透明，SystemTime仍可能随系统日期调整。

三个类型的固有方法（inherent methods）、运算符（operators）和特征实现（trait implementations）均由原类型提供。原有Option、标准Result、恐慌（panic）条件及平台／宿主（platform/host）边界保留，本模块不新增可失败操作。后续本库拥有的可失败接口统一使用pi_result，不因公开兼容名称改变标准库原有签名或失败语义。

重导出本身不采样，不创建实例、缓存（cache）、全局对象（global object）或维护线程（maintenance thread），没有初始化、关闭或取消操作。调用标准Instant::now或SystemTime::now仍执行标准取时，不读取本库手推缓存；这些方法的可用性与成本由原标准库及宿主决定。没有包装工作不等于底层取时无成本，也不形成高频读取的性能证据。

已决定直接重导出，理由是保留标准身份和完整既有接口而无需维护转发。自有包装类型（newtype wrapper）可以另定方法和错误规则，但身份不同且需要转发与转换，本单元不选。调用者自行导入std同样合法且可直接组合，但不提供本库统一兼容入口；保留这一用法不需要另建适配库。

@to_design：后续手推或自定义时间点与Instant／SystemTime的等价转换，仍需确定起点映射（origin mapping）、计量域相容性（measurement-domain compatibility）、范围及精度条件。当前不实现该映射，不能从裸纳秒值推定成功；这一未知不影响三个标准重导出名称的类型身份。

## 平台单调来源（platform monotonic source）

历史状态（historical status；以下仅保留原占位阶段，当前平台来源五个函数体均已实现）：pi-clock-platform的[lib.rs](../crates/platform/src/lib.rs)公开monotonic模块（module）；壳（shell）[monotonic.rs](../crates/platform/src/monotonic.rs)定义类型（types）、固定基准（fixed basis）BASIS、四个公开方法（public methods）与prepare_once／prepare_basis／sample_raw／elapsed_raw四个私有函数（private functions），八处函数体（function bodies）均为单表达式委托（single-expression delegations）。文件末尾通过include!("monotonic_inner.rs")纳入芯（inner）[monotonic_inner.rs](../crates/platform/src/monotonic_inner.rs)；二者属于同一Rust模块（module），共享导入（imports）与私有定义（private definitions），不建立implementation子模块（submodule）。芯（inner）的八个配对函数（paired functions）保持私有（private），其中五个仍为todo!。平台库依赖pi-clock-core的来源契约和标准Duration，并使用pi_result；基础库不反向依赖平台库或具体运行时（runtime）。以下描述实际声明及占位注释所确定的合同，不表示构造、采样或间隔算法已可运行。

### 表示与所有权（representation and ownership）

公开PlatformMonotonicSource的唯一字段basis为私有的&'static PreparedBasis；它派生Clone／Copy。复制只共享固定借用，不重新准备或取时，不克隆引用计数，也不复制时钟的起点、缓存或推进状态。公开PlatformMonotonicSample仅有私有raw: RawSample，派生Debug／Clone／Copy；复制样本不重新采样或分配身份，Debug不构成通信编码（wire encoding）。两者均没有公开原始值构造入口；来源没有Default、关闭、重置或重新绑定操作，样本没有公开排序、数值转换或反序列化入口。

私有PreparedBasis保存capabilities: SourceCapabilities；Windows额外保存正频率NonZeroU64，合格浏览器配置额外保存实际Performance对象的JsValue及now函数的js_sys::Function。私有RawSample按目标保留Unix的i64秒与u32纳秒、Windows的i64计数、WASI预览1／2的u64计数或浏览器的f64毫秒。字段及目标条件以monotonic.rs为准，不引入统一裸纳秒表示或独立样本身份。

静态BASIS的类型为OnceLock<PreparedBasis>，只为成功基准留位置，不保存Result，不提供take、替换或可写状态出口。一个确定拥有者的来源家族（source family）共享固定基准；基准发布后不变，丢弃来源句柄不使它或既有样本失效。公开样本的相容性由固定基准及私有构造出口共同建立，不凭对象地址或数值相等判断，不把独立库副本、Wasm实例、机器或重启后的计数纳入同域承诺。

### 准备、采样与间隔（preparation, sampling and elapsed duration）

try_prepare的合同是显式准备或复用基准。已就绪时只返回共享句柄；首次尝试先在发布同步之外准备元数据并探测一次真实读数，探测不产生公开样本，也不建立上层时钟的起点。准备或探测失败返回pi_result并释放本地候选，后续可显式重试。并发首次尝试可各自准备，只有一个成功候选发布，其余成功者释放候选并复用胜者；某次Err不证明基准仍为空，也不撤销其他调用的成功。

MonotonicSource实现的Sample为PlatformMonotonicSample。try_sample通过source_sample_inner只使用句柄的固定基准，将sample_raw的成功值包装为不透明样本；try_elapsed通过source_elapsed_inner借用两个样本并交给elapsed_raw；capabilities通过source_capabilities_inner复制保存的描述值，不采样、查询宿主或启动维护。正常域内没有关闭／重绑引起的样本失效。来源不拥有上层时钟的相对起点、缓存、等待队列（waiting queue）或后台线程（background thread）。

历史状态（historical status；以下仅保留原占位阶段，当前平台来源五个函数体均已实现）：五个私有实现入口仍为todo!，当前执行相应路径会恐慌（panic），不能当作可用取时服务。各入口的实现意图如下，不将这些意图写成已验证行为：

| 私有入口（private entry） | 职责与保持条件（responsibility and preservation） |
| --- | --- |
| source_prepare_inner | 组合BASIS、prepare_basis及一次sample_raw探测；只有取得静态槽内引用才构造公开来源句柄，不以探测值建立时钟起点。 |
| prepare_once_inner | 借用槽的生命周期决定返回引用；FnOnce构造／探测边界为静态分发（static dispatch）。已就绪时不调用二者，否则各执行一次后set一次，竞争败者释放候选并复用胜者；不保存失败、不自旋重试，不依赖get_or_try_init。局部槽仅供私有协议验证，不产生第二条公开生产基准。 |
| prepare_basis_inner | 准备固定能力与目标元数据，不发布。Windows保存经成功状态及正值检查的QPF；浏览器捕获同步属性异常，检查并保留实际对象与函数，不动态构造函数或重新绑定来源。 |
| sample_raw_inner | 借用既定基准完成一次真实采样，校验原始返回值；不换源、不分配样本身份、不克隆共享句柄、不取得管理锁或发布上层缓存。 |
| elapsed_raw_inner | 校验载荷、顺序及可表示范围，先求原始直接差再换算。使用受检宽整数（checked wide integers）及标准秒／纳秒表示，浏览器保留已声明浮点误差；不采样或修改参数，不提前缩窄到u64总纳秒。 |

Unix与WASI按整数直接差计算；Windows先扩宽计数作差，再受检乘十亿、除固定正频率并向下取整，不使用近似整数每计数因子。浏览器先对有限非负f64毫秒作差，再换算并floor，承认IEEE浮点舍入（floating-point rounding），不承诺实数意义上的精确向下取整。浮点转整数前须确认有限、非负且严格小于2^128，随后用整数商余数构造标准Duration并检查秒范围；不允许强制转换静默归零或饱和。两端不能先各自量化，同一有效样本的成功间隔可以为零。

### 目标与宿主边界（target and host boundaries）

[Cargo.toml](../crates/platform/Cargo.toml)默认功能特性（default features）为空，browser仅为wasm32／unknown目标引入可选wasm-bindgen 0.2及js-sys 0.3；Linux／Android依赖rustix 1.1，Windows依赖windows-sys 0.61，WASI预览1依赖wasip1 1.0、预览2依赖wasip2 2.0。按操作系统保留的声明及依赖不证明所有架构或宿主均获支持。

| 来源家族（source family） | 已声明来源与范围（declared source and scope） | 休眠累计行为（suspend behavior） |
| --- | --- | --- |
| Linux／Android | CLOCK_MONOTONIC；GNU Linux x86_64要求内核3.2及glibc2.17以上，aarch64要求内核4.1及glibc2.17以上；Android aarch64要求API21以上。 | Excludes |
| Windows | QPC原始计数与准备时的QPF；Windows x86_64，Windows10／Server2016以上。 | Includes |
| WASI预览1／2 | 按target_os=wasi及target_env=p1／p2选择原始纳秒出口；预览2要求宿主提供wasi:clocks/monotonic-clock@0.2.12或经验证的兼容映射。 | Unspecified |
| 浏览器（browser） | 显式browser功能，wasm32／unknown且未启用atomics；仅可信、合格、固定JavaScript域，单线程（single thread）且无共享内存（unshared memory）。 | Unspecified |

浏览器的构建条件不认证完整宿主装配，也不因自动特征（auto traits）而允许跨worker使用；不自行添加unsafe Send／Sync实现。启用atomics或未启用browser的unknown配置不使用内置浏览器后端，不支持目标的预期准备结果为Unsupported；当前占位尚未实现这些返回。原生支持配置允许跨线程并发使用；其他目标不能因类型声明存在就当成运行支持。

### 错误与成本（errors and cost）

准备合同将不支持目标或缺少合格浏览器能力归为Unsupported；普通系统／绑定调用失败及无效采样数据归为Dependency并保留原因。间隔的载荷不相容、逆序、无效数值或超出标准Duration范围归为InvalidInput，不静默归零、回绕或饱和。普通失败统一pi_result，不用unwrap／expect、系统日期或模拟值冒充成功；合法零计数仍可成功。

浏览器使用捕获同步异常（synchronous exceptions）的属性读取及固定函数调用，JavaScript异常值作为attach_opaque附件保留，并附静态操作说明，不为包装错误再次调用任意JavaScript格式化。WASI缺失导入发生在实例化（instantiation）阶段，宿主陷阱（host traps）及进程中止不被包装成可恢复错误；预览2无Result的取时出口保留此边界。预览1请求1纳秒精度不构成硬件准确度保证，浏览器降精度（precision coarsening）也不被换算消除。

准备可能访问系统／JavaScript并分配，提交可能短暂等待其他发布者，但不在发布内部同步期间执行准备或探测；正常采样不逐次创建身份、克隆共享句柄或扫描队列。系统、绑定及错误构造成本仍须另计，不承诺整个依赖路径等待自由（wait-free）、纳秒分辨率或固定耗时。来源句柄可共享仅为来源复用，不代替上层时钟的起点管理、独立分叉、刷新发布或期限通知。


## 全局手推核心声明候选（global manual-core declaration candidate）

候选源码根为crates/manual，模块为global；壳（shell）为src/global.rs，配对芯（inner）为src/global_inner.rs，同一实际模块在壳尾include!。当前声明候选先保存在隔离工作目录，未写入产品源码根，未实现函数体。

已选择单个AtomicU64缓存、Rust1.95的try_update纯闭包条件提高、SeqCst读取与更新、私有OnceLock成功安装、固定真实首样本为零起点。同步与原生异步刷新共用同一行为，异步首次轮询执行，无挂起或运行时依赖。候选公开名称为GlobalManualInstant、RefreshOutcome、global::try_init/try_now/try_refresh/try_refresh_async/try_capabilities；显式初始化失败可重试，成功后重复为AlreadyExists。

GlobalManualInstant为私有u64字段的新类型，提供ZERO/MAX、标准Duration的try_from_elapsed及duration_since_origin、try_add/try_sub/try_duration_since。纯运算不访问全局状态；输入范围或逆序错误为InvalidInput。RefreshOutcome按值返回sampled/published/advanced，字段修改不影响缓存。固定全局状态包含source、origin和cached_nanos；私有泛型槽只帮助同一生产协议的局部验证，不是公开多实例或来源替换。

未初始化访问InvalidState；来源错误原样传播；真实经过总纳秒超过u64为ResourceExhausted。本次失败不写缓存，其他并发调用仍可能提高。普通成功读取只检查槽并加载一次缓存，不能把SeqCst时钟读数当成任意用户数据同步或已测量的5ns保证。

标量缓存参考quanta0.13的GLOBAL_RECENT/recent，但不能照搬其普通store；条件更新复用Rust标准库，不引入ArcSwap。多字段不可变配置未来可另行选择ArcSwap，本次没有该需求。

本批13个业务占位分别承担5项纯时间运算、4项全局入口装配和4项局部安装/首样本/读取/刷新；常量及简单委托由外部装配。14处壳委托分别连接对应局部入口；外部已提供隔离目录/tmp/pi-clock-manual-design-20261112/candidate中的Rust1.95 cargo build -p pi-clock-manual --offline退出0、cargo test -p pi-clock-manual --doc --locked --offline退出0（一项no_run用法编译，未执行业务）、cargo doc -p pi-clock-manual --no-deps --locked --offline退出0；修补版Dylint6.3.2经Linux驱动接口执行cargo +nightly-2026-07-19 check -p pi-clock-manual --locked --offline退出0。七组跨目标库编译也退出0，仅静态库证据；构建/壳检查保留23条具体占位相关警告（原始日志另含1条汇总行），无检查豁免，未执行手推业务、性能或安全测试。自动推进封装同列优先，接续消费同一全局公开刷新，不在本主题增设线程控制API。


## 当前实际手推源码（current manual source files）

当前实际源码根为crates/manual，公开库入口为src/lib.rs，src/global.rs在文件尾include!("global_inner.rs")，两文件同一实际模块。原隔离目录保存前述候选原文；实际文件已生成，签名／字段／14处简单委托保持，13项业务todo仍未实现。

本项目路径实际命令cargo +1.95.0 build -p pi-clock-manual --locked --offline已退出0，保留23条具体占位警告及1条汇总行；这不是业务执行或性能结果。普通成功读取只检查安装槽和加载单个缓存，错误构造成本另计。后续须实现的函数义务仍以实际global_inner.rs中的原始完整文字为准。
