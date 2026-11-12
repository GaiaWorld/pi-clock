// 配对芯（paired inner）：由auto_refresh.rs尾部include!装配到同一模块。

// # 用途
// 同步启动（synchronous startup），返回实际间隔（effective interval）。
fn start_global_inner(interval: Option<Duration>) -> Result<Duration> {
    todo!(r#"目标（goal）：同步启动（synchronous startup），返回实际间隔（effective interval）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：先global::try_capabilities验证显式安装，不隐式初始化/刷新；normalize_interval后调用try_start_on及spawn_ready。就绪成功仅说明线程曾准备完成；第一轮仍先完整等待。全部公开Err附ControlEffects；普通准备错误原样保留，可回收清理后重试。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 查询可成立的状态快照（status snapshot）。
fn status_global_inner() -> AutoRefreshStatus {
    todo!(r#"目标（goal）：查询可成立的状态快照（status snapshot）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：只调用snapshot_on，不采样、不取走退出结果、不修改状态。快照只保证本次查询期间成立过，不能代替下一控制操作的资格检查；Finished不等同TLS析构结束。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 提交停止请求（stop request），不等待退出（exit）。
fn request_stop_global_inner() -> Result<bool> {
    todo!(r#"目标（goal）：提交停止请求（stop request），不等待退出（exit）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：使用CONTROL与本模块notify调用request_stop_on；返回是否首次提交。允许已经进入的一轮刷新完成；保留pi_result及ControlEffects，不追加启动/隐式回收。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 停止并等待退出（stop and join），唯一取得完整结果（complete result）。
fn stop_join_global_inner() -> Result<bool> {
    todo!(r#"目标（goal）：停止并等待退出（stop and join），唯一取得完整结果（complete result）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：首先AUTO_WORKER标记拒绝self-join为InvalidState且无副作用，优先于Busy；再调用stop_join_on。空槽false，实际回收true；通知失败仍join并合并同次完整错误。worker_joined=true的返回错误也表示已确认退出且本次结果已交付，可显式重启。这是同步阻塞入口，不在要求不阻塞的任务执行上下文直接调用。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 完整时长规范化（duration normalization）。
fn normalize_interval_inner(interval: Option<Duration>) -> Duration {
    todo!(r#"目标（goal）：完整时长规范化（duration normalization）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：None或interval小于100µs/大于1s返回1ms，端点原样保留。完整Duration比较，含0及Duration::MAX；不夹紧、不溢出、不取时或分配。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 同一生产唯一启动协议（single-start protocol）。
fn start_on_inner<'a, F>(slot: &'a ControlSlot, interval: Duration, build: F) -> Result<Duration>
where F: FnOnce(&mut StartupGuard<'a>) -> Result<()> {
    todo!(r#"目标（goal）：同一生产唯一启动协议（single-start protocol）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：先观察非空槽拒绝AlreadyExists且不调用build；空槽调用create_run并CAS None->本Arc，竞争败者释放空候选且不调用build。成功后StartupGuard拥有Starting资格，锁外build完成资源/线程就绪。成功先把完整Owned资源发布到run.resources，再保留STOP/BODY_DONE以CAS提交Owned，解除保护。普通失败未生成线程则仅CAS本Arc->None；已有线程必须CAS Starting->Collecting|STOP，同时从成功旧字计算notify_needed，转交完整句柄/错误给已armed的CollectionGuard再解除原保护，统一collect_owned。任何可重试CAS闭包只处理固定值，不放build/通知/采样/分配。staged资源保证提交前展开不丢句柄；提交Owned后的旧StartupGuard不得再取回资源。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 一致状态投影（consistent state projection）。
fn snapshot_on_inner(slot: &ControlSlot) -> AutoRefreshStatus {
    todo!(r#"目标（goal）：一致状态投影（consistent state projection）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：load_full取得本Arc，单次SeqCst加载state，再load_full确认仍同一Arc，否则重新观察；None可直接返回Stopped/None。同次状态字投影Starting、Owned派生Running/StopRequested/Finished（BODY_DONE优先）、Collecting为Joining、Completed为Finished、ExitUnconfirmed。interval取同一不可变Arc。不得拼接独立原子字段或把快照当操作权限；不消费AtomicTake，不调用JoinHandle::is_finished，不取内核时间。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 停止原子发布（atomic stop publication）及通知（notification）。
fn request_stop_on_inner<N>(slot: &ControlSlot, notify: N) -> Result<bool>
where N: FnOnce(&Poller) -> Result<()> {
    todo!(r#"目标（goal）：停止原子发布（atomic stop publication）及通知（notification）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：load_full固定本周期；空槽false，Starting Busy，Collecting/Completed false，ExitUnconfirmed Corrupted且不消费诊断。Owned若STOP已有则false；否则CAS同一state只置STOP，保留阶段及BODY_DONE。只成功CAS的首次请求调用一次notify；若CAS见阶段变化按新阶段返回或见BODY_DONE保留重试，不修改别人资格。notify在CAS之后且重试闭包之外；失败不回滚，附ControlEffects(true,false)。旧Arc延后通知只作用本运行。未取得运行或未执行控制的错误不得虚构本次效果。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 同一生产唯一回收协议（single-collector protocol）。
fn stop_join_on_inner<N>(slot: &ControlSlot, notify: N) -> Result<bool>
where N: FnOnce(&Poller) -> Result<()> {
    todo!(r#"目标（goal）：同一生产唯一回收协议（single-collector protocol）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：先自身标记拒绝，空槽false；Starting/Collecting Busy。准备original=None且记录origin的未armed CollectionGuard；Owned用一次SeqCst CAS同时提交Collecting|STOP并授予唯一回收权，保持BODY_DONE，notify_needed由成功旧STOP计算；成功后立即armed，随后才load_full当前run.resources并从Owned记录AtomicTake一次移交完整句柄/控制错误；不得把CAS前预读资源跨Owned/Collecting/Owned阶段往返使用，不留可恐慌的未受保护转移窗口。败者不置STOP、不取资源、不调用notify。Completed同样先CAS唯一资格，成功后才加载当前记录并取走已有结果，worker_joined=true，不再join；ExitUnconfirmed只以AtomicTake一次领取完整诊断，以后仍Corrupted，永不清槽。胜者统一调用collect_owned；未知/缺失内部资源必须保留资格并报告Corrupted，不能伪造成功。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 构造空运行记录（empty run record）。
fn create_run_inner(interval: Duration) -> Arc<RunControl> {
    todo!(r#"目标（goal）：构造空运行记录（empty run record）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：interval已规范化；构造全新Arc，state为Starting且STOP/BODY_DONE清零，poller/resources为空。只分配运行记录，不创建内核资源、线程或通道，不采样global，不分配外部身份。Arc分配失败不承诺可恢复pi_result错误；不把系统取时放此路径。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 原生资源准备与就绪握手（native preparation/readiness handshake）。
fn spawn_ready_inner(attempt: &mut StartupGuard<'_>) -> Result<()> {
    todo!(r#"目标（goal）：原生资源准备与就绪握手（native preparation/readiness handshake）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：本保护拥有Starting。Poller::new错误按pi_result IO分类保留；成功Arc发布到run.poller后创建oneshot::channel<()>，仅启用std、不启用async。Builder固定无NUL线程名创建worker；recv前JoinHandle存入attempt。recv为已确认的同步阻塞，断连失败交由启动协议统一停止/join；不detach。收到ready后线程随后失败不否定已完成握手。就绪通道只一次创建/发送/接收，不能进入每轮刷新。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 单一工作线程（worker thread），独占内核等待（kernel waiting）。
fn worker_inner(run: Arc<RunControl>, ready: oneshot::Sender<()>) -> Result<()> {
    todo!(r#"目标（goal）：单一工作线程（worker thread），独占内核等待（kernel waiting）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：首先设置AUTO_WORKER，然后立即安装WorkerExitGuard，覆盖正常返回与panic展开。从run.poller一次load_full取得固定Poller，创建Events后发送ready，失败Cancelled退出；随后调用run_loop(&run.state,interval)，等待闭包仅wait_until，刷新闭包仅global::try_refresh映射单位值。工作循环不读全局CONTROL或run.resources，不逐轮Arc复制。退出保护发布BODY_DONE只是主体结束，不代替join。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 固定延迟循环（fixed-delay loop）。
fn run_loop_inner<W, R>(state: &AtomicU8, interval: Duration, wait: W, refresh: R) -> Result<()>
where W: FnMut(Instant) -> Result<()>, R: FnMut() -> Result<()> {
    todo!(r#"目标（goal）：固定延迟循环（fixed-delay loop）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：每轮SeqCst检查STOP；用真实std::Instant::now.checked_add(interval)构造期限，失败ResourceExhausted且不进入Waiting。每次等待前后和刷新前检查STOP；空返回未到期保持原deadline，不能视为到期。wait闭包只在原始Interrupted处理后返回，本层不把pi_result Cancelled当Interrupted。到期且未停止仅一次refresh；其他错误原样退出，成功才开始下一完整间隔，不补跑、不长期忙等。W/R为同一私有生产算法的可控验证接入，不是公开运行时trait。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 单一内核期限等待（single kernel deadline wait）。
fn wait_until_inner(poller: &Poller, events: &mut Events, deadline: Instant) -> Result<()> {
    todo!(r#"目标（goal）：单一内核期限等待（single kernel deadline wait）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：clear并复用Events，调用Poller::wait_deadline；仅原始std::io::ErrorKind::Interrupted继续同deadline，其余IO错误经pi_result保留。事件数量不是到期证明；只有工作线程可调用wait，不注册用户FD、不新增unsafe，不sleep轮询或零超时补跑。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 最佳努力通知（best-effort notification）。
fn notify_inner(poller: &Poller) -> Result<()> {
    todo!(r#"目标（goal）：最佳努力通知（best-effort notification）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：仅调用一次Poller::notify并保留IO错误；STOP及控制资格在调用前已原子提交，通知不改变时间/资格。失败后的再次notify不一定重发，Ok不证明线程已处理；不在可重试CAS闭包内通知。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 完整报告合并（complete report combination）。
fn merge_errors_inner(primary: Option<Error>, secondary: Option<Error>) -> Result<()> {
    todo!(r#"目标（goal）：完整报告合并（complete report combination）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：无错误Ok，单个原样，多个以error-stack的expand/push/change_context保留所有frames及阶段附件，恢复首要ErrorKind。线程错误优先，通知/启动清理错误次要。不可Clone报告，不只拼字符串，不吞掉任一分支；仅错误路径分配。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 已确认退出结果转换（confirmed-exit conversion）。
fn finish_join_inner(raw: std::thread::Result<Result<()>>) -> Result<()> {
    todo!(r#"目标（goal）：已确认退出结果转换（confirmed-exit conversion）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：join外层Ok透传线程pi_result结果；外层Err为已确认线程panic，提取str/String或非字符串说明构造Internal，原panic负载按所有权释放。join返回的panic结果不同于join调用自身展开；不捕获abort/进程退出，不公开Box<Any+Send>。调用前guard必须已标记worker_joined=true。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 共享启动失败及正常退出回收（shared exit collection）。
fn collect_owned_inner<N>(guard: CollectionGuard<'_>, notify: N) -> Result<bool>
where N: FnOnce(&Poller) -> Result<()> {
    todo!(r#"目标（goal）：共享启动失败及正常退出回收（shared exit collection）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：前提为guard已唯一占有本Arc的Collecting且STOP已置；原保护已解除或资源仍由guard.original保护。只notify_needed时通知，失败合并control_error仍继续join。join前join_started=true，返回后立即worker_joined=true，再finish_join并保存outcome；已有Completed结果不再join。合并完整结果及ControlEffects(true,true)，先把本次结果安全移到返回对象，再CAS全局本Arc->None并解除保护，最后交付；本次清理不得清其他Arc。CAS/组装阶段若展开守卫恢复完整资源/结果；常规错误也是已交付结果，允许后续显式重启。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 启动展开恢复（startup unwind recovery）。
fn drop_startup_inner(guard: &mut StartupGuard<'_>) {
    todo!(r#"目标（goal）：启动展开恢复（startup unwind recovery）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：未armed直接结束。若本Arc尚Starting且无线程/无staged句柄，只CAS本Arc->None；若已有线程，先CAS Starting->Collecting|STOP并从成功旧字取得notify_needed，在本保护仍持有该排他资格时从thread或staged记录接管完整句柄，执行必要notify并合并错误，然后先发布完整Owned资源再CAS提交Owned（STOP保持、BODY_DONE保留）。不能先公开Owned再补通知错误，否则会与另一回收者竞争。若已经提交Owned/Collecting，原保护不再取回资源，不能抢走别人的句柄或结果。Drop不直接join、不detach、不清他人资格；恢复失败保留本Arc并以退出未确认拒绝重启。异常Drop不能在已有展开期间二次恐慌。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 回收展开恢复（collection unwind recovery）。
fn drop_collection_inner(guard: &mut CollectionGuard<'_>) {
    todo!(r#"目标（goal）：回收展开恢复（collection unwind recovery）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：未armed直接结束。只恢复本Arc且当前Collecting归本保护：优先按guard本地完整thread/outcome恢复；只有本地无thread/outcome且尚未从Owned/Completed领取时，original尚None才在Collecting下加载当前resources。origin=Starting且本地已有句柄、resources仍None是合法启动清理，不能误报缺失。依据origin与实际记录恢复；原Owned记录尚未take则保留原记录恢复Owned，原Completed记录尚未take则保留真实结果恢复Completed，不能用Internal替换未领取的报告。已取得且仍有句柄则先发布完整Owned记录再提交Owned。worker_joined且已有outcome时先发布Completed记录再提交Completed；只有真实join后转换意外展开、outcome确未产生时才用Internal。发布Completed必须先把guard.control_error完整合并进outcome或Internal；发布Unconfirmed也必须合并既有通知/启动错误，不能让无独立control_error字段的记录丢弃这些报告。join已开始、句柄已消耗且未确认则先发布Unconfirmed完整诊断再提交ExitUnconfirmed；无资格释放。状态CAS保留并发BODY_DONE且STOP永不清零；资源在阶段提交前发布，不把被取空的旧记录重新当完整资源。不得二次恐慌、join、重启或清他人Arc。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

// # 用途
// 工作主体完成发布（worker-body completion publication）。
fn drop_worker_inner(guard: &mut WorkerExitGuard<'_>) {
    todo!(r#"目标（goal）：工作主体完成发布（worker-body completion publication）。
前提与依赖（preconditions/dependencies）：遵守配对声明的参数、私有数据、阶段及唯一权限。
行为与边界（behavior/boundaries）：工作线程入口立即装入本保护，正常返回/可展开panic都以SeqCst fetch_or(BODY_DONE)发布一次；保留STOP及阶段位，不触碰全局槽/资源，不分配、不notify、不join、不加锁。BODY_DONE不证明TLS析构完成，正常因panic返回join仍须完整回收；abort不保证Drop运行。
成本与验证（cost/verification）：不得改动global取时热路径；CAS闭包无系统调用/分配/回调；以真实生产出口及同一私有协议验证正常、边界、错误和并发后继。不提供公开复位/来源注册，不引入运行时依赖。"#)
}

