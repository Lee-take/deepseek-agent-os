# DS Agent v1.0 目标架构与迁移总图

状态：实施基线
日期：2026-07-12
适用范围：v0.5 自动任务、v0.6 连接器到底层 v1.0 普通用户可信 Agent

## 1. 架构结论

v1.0 允许结构性重构，但不允许重写安全语义。目标不是继续扩大
`commands.rs`、`App.tsx` 或制造第二套执行引擎，而是把已经验证的权限、证据、恢复、
凭据、连接器和 DeepSeek 模型边界收敛为可组合的 Kernel 服务。

不可破坏的四条主线：

1. DeepSeek 负责理解、规划和基于证据生成回答；本地 Kernel 决定能力是否存在、是否授权、
   是否可恢复以及结果是否被验证。
2. 所有副作用继续通过 Tool Runtime、精确审批、资源租约、幂等键、证据和恢复状态执行，
   自动任务不得获得更宽权限。
3. Provider 内容永远是未信任证据；邮件、日历、附件、错误正文都不能成为系统指令、
   权限声明或模型边界。
4. 凭据、审批、连接器 continuation、绝对本地路径和敏感内容不进入普通 DTO、事件、日志、
   导出、会话或模型上下文。

## 2. 目标分层

```text
普通用户界面
  ├─ Chat / Task / Automation / Review / Recovery
  └─ 只消费 secret-free projection，不持有 provider 协议状态

薄适配层
  ├─ Tauri commands
  └─ 输入解析、DTO 转换、调用 Kernel；不实现业务状态机

DS Agent Kernel
  ├─ Agent Run：队列、租约、取消、Subagent、恢复
  ├─ Tool Runtime：能力、资源、审批、执行、验证、证据
  ├─ Automation：触发窗口、错过策略、重试、检查点、Review
  ├─ Connector Runtime：账号、capability、generation、调用、同步
  ├─ Credential Vault：opaque handle、DPAPI、原子替换、单飞刷新
  ├─ Safe Landing：流式下载、内容判定、隔离、保留、清理
  ├─ Evidence：来源、hash、未信任标记、可回放 receipt
  └─ Event Store：事务投影、CAS、append-only 审计

Provider adapters
  ├─ Microsoft Graph
  ├─ 后续 Google / 本地文件 / 其他 provider
  └─ 只做 typed request、固定 origin、DTO normalization、错误归一化

DeepSeek model boundary
  └─ 接收最小上下文和已验证证据；不直接接触 token、原始 provider transport 或本地路径
```

## 3. 状态所有权

| 状态 | 唯一所有者 | 禁止事项 |
| --- | --- | --- |
| Agent/Automation/Review/Tool 状态 | SQLite 事务投影 + Kernel event | 前端自行推进状态 |
| Connector account/generation/sync | Connector + Event Store | provider adapter 直接写库 |
| Access/refresh token | DPAPI 当前用户保险库 | SQLite、event、IPC、日志持有 token |
| Provider continuation | 专用 sync state 表 | 审计事件或模型上下文持有 continuation |
| 下载中的附件 | Safe Landing 临时区 | 写入用户任意路径或自动打开 |
| 已验证附件证据 | 受管 workspace + secret-free receipt | 保存绝对路径到普通前端持久化 |
| 模型计划 | Agent Run | 模型绕过本地 capability/approval |

## 4. 已完成的底座迁移

- 自动任务使用持久化触发窗口、Agent run、检查点、Review 和恢复，不再依赖页面生命周期。
- 连接器使用 provider-neutral typed contract、固定网络 origin、secret-free account DTO、
  generation CAS、增量同步和有界保留。
- Provider 元数据与执行能力已经拆开：`ConnectorProvider` 只声明稳定 identity/capability，
  typed mail/calendar/sync、草稿、副作用和只读 reconciliation 使用各自窄 trait。旧的无请求参数
  通用 read 已删除；shared contract coverage 要求每个 advertised capability 恰好由一个真实 runner
  覆盖，metadata runner 不产生 provider 副作用，空搜索结果也不再被误判为 contract 失败。
- 断连改为 `DisconnectPending` 两阶段状态：SQLite 先记录意图并增长 generation，保险库删除后
  再以 ticket CAS 完成；启动 sweep 可恢复两个崩溃窗口。
- Windows 凭据从 2560 字节的 Generic Credential 单 blob 迁移为 opaque handle 对应的
  64 KiB DPAPI 当前用户密文文件；写入使用同目录临时文件和原子替换。
- Microsoft 授权码交换和刷新使用独立固定 token endpoint 客户端；不扩展 Graph GET transport，
  不发送 client secret 或 bearer header，不跟随重定向，响应上限 64 KiB。
- Microsoft scope 校验把 `offline_access` 当成协议 scope，access scope 仍要求精确集合一致；
  refresh rotation 原子替换，响应缩权或扩权均失败并保留旧 envelope。
- 首个 `commands.rs` 结构切片已完成：应用更新版本解析、可信 release 选择、下载、路径校验和
  安装调度归入 `kernel/app_update.rs`，三个原名 Tauri command 只保留在薄适配器
  `app_update_commands.rs`。Agent 侧的 Tool contract、精确审批、资源租约、证据、审计和恢复仍由
  原执行链负责，没有因迁移改变风险等级或 IPC 语义。`commands.rs` 已降到 CodeGraph 1 MiB
  上限以下并重新进入依赖图。
- 该迁移不掩盖旧 updater 的独立安全债。进入 v1.0 发布硬化前，下载必须增加不跟随重定向、
  Content-Length 与流式字节上限、同目录原子落盘、hash/文件身份绑定和 opaque download receipt；
  安装必须按 receipt 重新验证，不能继续把可预测临时目录中的任意绝对路径当作充分授权。

## 5. 后续迁移波次

### Wave A：附件 Safe Landing

建立独立于现有本地聊天附件 staging 的连接器下载边界：

- metadata 与 bytes 分离；未经精确审批不获取 `$value`；
- 固定 provider origin，流式读取，单文件 20 MiB 硬上限；
- 规范化文件名，受管临时目录，禁止 symlink/reparse point、路径穿越和任意绝对路径；
- 扩展名、声明 MIME、检测 magic 三方一致；可执行、脚本、宏、歧义和 archive bomb 失败关闭；
- 下载 receipt 绑定 provider、account、message/event、attachment、generation、审批指纹、hash、
  字节数和类型；断连后的晚到下载不能原子落盘；
- 默认不打开、不执行、不放入模型上下文；只有后续显式读取动作才能形成证据；
- 临时文件、失败隔离和已完成文件都有有界保留与启动清理。

当前最小持久化切片已经把 `connector.attachment.download` 建成 Critical
ToolContract，并以 exact request fingerprint、可见安全预览、一次性审批消费、
账户 generation 和持久 workspace identity 绑定 reservation。文件提交采用
`reserved -> ready -> completed`：先流式写入并验证 `.part`，再持久化冻结 receipt，
然后 rename，最后以 connected/generation CAS 完成 Tool 和安全 receipt 事件。
任一失败先取得 `cleanup_required` 所有权并立即把 Tool 终态化；启动在 worker 之前
按批清理持久 reservation，workspace identity 不匹配则进入 `repair_required`，不猜测
或删除未知位置。断连在 generation 递增事务内使旧 generation 的 `reserved/ready`
失效。调用方已不能提供 account、metadata、workspace 或 generation closure；Microsoft
执行器只能从 Event Store reservation 重取权威执行上下文。

2026-07-12 的结构重构进一步完成了以下底座：

- `capability_access_state`、`tool_invocation_state` 与一次性消费表成为 indexed current
  projection；append-only Kernel events 只承担审计/迁移，event 与 projection 同事务，
  event-id 去重保证重放不重复推进 revision；
- exact preview 加入 renderer revision、domain-separated hash 与 request row revision。
  通用 catalog、execute、authorization、ready 和 resolver 均拒绝附件；专用 prepare、
  approve+reserve 与 reject saga 各自在单一 SQLite 事务内完成状态、Tool 和隐私清理；
- raw provider refs 只短期存在于 pending/active source 表。durable landing 从 reservation
  开始只存 redacted metadata；进入 `ready` 后 active source 立即删除；
- Windows Safe Landing identity 升级为 `v2(path + workspace FILE_ID + landing FILE_ID)`。
  workspace/landing 目录句柄在操作期间禁止 delete-share，reparse point 失败关闭；
  `.part` 创建、hash/type/Office 验证和 rename 使用同一文件句柄，receipt 持久化
  volume+FILE_ID，精确删除必须重新打开 no-reparse handle 并匹配该身份；
- `ready` 启动恢复会区分 temp 尚未 rename 与 final 已 rename 两个崩溃窗口，重新校验
  identity/hash/size/type 后补 rename 或补 completed；冲突进入 repair，瞬时不可用保留
  ready 重试，不再把 uncertain commit 当作普通失败删除；
- `.part` 创建后、首字节写入前先持久化 `staging` FILE_ID checkpoint；completed 默认
  30 天过期，单 workspace 同时最多 32 项/256 MiB。运行期 worker 每 30 秒续扫，
  transient cleanup 使用持久指数退避，成功 retention 只删精确文件并保留安全 receipt；
- Office OPC 解析 `[Content_Types].xml`、根 officeDocument relationship 和全部 `.rels`；
  主文档 part/type 必须匹配，malformed XML、DTD/PI、外部或 URI target 均失败关闭。
- Recovery Center 只读取 path/token/raw-ref-free current projection。附件“安全重试”绑定
  Kernel 生成的 action fingerprint，并在同一事务写入 secret-free retry audit；它只排队
  `repair_required -> cleanup_required` 的 FILE_ID 精确清理，不重新授权、不调用 provider、
  不把 Tool 恢复为 Running；
- 启动恢复与 30 秒运行期 worker 已拆分：只有应用启动、connector worker 尚未开始时才可
  领取遗留 `reserved/staging`；运行期只处理已到期的 recovery/retention。ready transient
  使用持久退避与 attempt-first 排序，避免旧锁定批次长期压住后续项目；
- 2026-07-13 将附件恢复所有权统一为持久化 300 秒 lease + opaque claim token。
  ready、cleanup 和 retention 在执行文件动作前按同 token 续租；complete、defer、repair、
  fail 都同时校验 landing id、状态、token 与未过期时间。错误 token、过期 token、旧进程
  token 均不能推进终态；过期后新 token 才能接管；
- startup ready 只领取未处于未来 backoff 的崩溃残留；runtime ready 只领取显式到期的
  deferred row 或 expired claim，绝不领取正常下载短暂出现的
  `claim = NULL + next_cleanup_at = NULL`。Windows 单实例锁在 Event Store 与启动恢复之前
  取得，因此 startup 可以撤销上一进程 token；claim/token/expiry 不进入 IPC、Recovery
  Center、事件或 DeepSeek 上下文；
- 没有 FILE_ID 的失败只在两个受管 basename 都以 no-reparse 探测确认不存在时自动完成；
  任一未知文件存在则保留并进入 repair，瞬时不可用继续退避。坏 Tool projection 按 row
  隔离到 path/token-free Recovery Center，不再使同一批其他清理饥饿；
- legacy schema column migration 改为先查 `PRAGMA table_info` 再执行并传播真实错误；缺少
  完整 identity receipt 的 legacy completed 文件不自动删除，而是进入 Recovery Center。

该能力仍故意不向普通 Tool catalog/IPC 暴露。lease/token、disconnect、reparse、瞬时文件
锁、retention、错误 token、过期接管、重启和跨批次防饥饿已有离线对抗测试。激活仍需
专用附件下载 UI/IPC 入口、完整产品评审，以及用户对 live Microsoft 账户验证的全新明确
决定；在这些门槛前 capability 继续不可达。

### Wave B：恢复中心与可解释状态

- 把 `repair_required`、`disconnect_pending`、`revocation_pending`、sync exhausted、
  reconciliation required 投影为普通用户能理解的恢复卡片；
- 恢复动作仍由 Kernel 产生精确 request fingerprint，不允许前端拼接状态跳转；
- 每个失败说明“发生了什么、是否产生外部副作用、下一步是什么”，不显示 provider 原始正文。

2026-07-13 已完成第一段可持久化实现：Recovery DTO 不再携带任意 `summary`、原始状态文本或
前端可拼接的 `can_retry`，而是使用有界的 `reason_code`、`external_effect_state`、
`next_step_code`、status 和 tagged action。UI 对中英文 copy 做类型穷尽映射，固定展示“发生了
什么 / 外部操作 / 下一步”，错误也只显示本地固定文案，不渲染 backend/provider 原文。

`connector_sync_streams` 中 `stopped = true` 的邮件/日历只读同步现在投影为
`sync_exhausted` 卡片。其 id 是 domain-separated hash 派生的稳定 opaque UUID；account id、
stream fingerprint、continuation、retry state、tenant、credential handle 均不进入 DTO。
reconciliation 卡片只查询 invocation 的 id/status/time current projection，不再为列表读取带
provider evidence 的完整 invocation JSON。账户、同步和 reconciliation 首版全部只读；唯一
可执行 action 仍是附件 `repair_required -> cleanup_required`，且只接受 Kernel 生成的
fingerprint，不能调用 provider、重新授权或恢复 Tool。

这不是宣称所有恢复 saga 已完成。`revocation_pending` 已有 Fake-only 的持久 generation-bound
ticket、300 秒 claim、远端调用前 `remote_call_started` checkpoint、typed outcome 和仅在远端
确认后执行的本地凭据删除；`known_not_applied` 才允许有界重试，`uncertain` 或启动发现的
`remote_call_started` 一律进入不可重放的 reconciliation required。本地删除失败或删除后崩溃
可以从 `remote_confirmed` 幂等恢复，包括凭据已不存在的窗口。旧的内存式 revoke 路径已删除。
该 Kernel worker 没有 Tauri 入口、Microsoft revoker、live registry 或 Recovery action，因而
revocation 卡仍保持只读。mutation reconciliation 尚无生产 provider，sync 也尚无可审计的
重新调度命令，因此三者都不能把说明卡伪装成恢复能力。附件 fingerprint 后续还需升级为独立 row revision、
account generation、request/landing binding 等完整动作状态的 opaque token；在此之前继续依靠
当前 failure kind、workspace/storage identity、updated-at CAS 和后续 FILE_ID/lease fencing。

进入 reconciliation worker 之前的 provider-neutral gate 已完成：Fake 与 Microsoft 离线 adapter
通过同一组 typed mail/calendar/sync runner，coverage 会拒绝未覆盖、重复覆盖和未宣告能力；
草稿与 mutation/reconcile 只能通过显式 side-effect runner。下一切片先把 account generation
冻结进新 mutation 的 Tool fingerprint/envelope/投影，再增加 reconciler-only registry 和持久
claim lease。legacy 无 generation 行保持可见但不得自动执行，worker 源码不得获得
`apply_mutation` 接口。

该下一切片现已完成为离线 Kernel 底座。新 mutation 必须把 account generation 写进 Tool
schema、fingerprint、immutable envelope 和 SQLite 投影；start 与 complete 均以当前 connected
generation 做事务 fencing。`reconciliation_required` 使用专用 transition、持久 due/backoff、
300 秒 opaque claim lease、renew、expired takeover 和 claimed completion。claim/renew/complete
都会核对 running Tool、一次性审批消费、Review 指纹、provider/account/capability 与 generation。
worker 只依赖 `ConnectorMutationReconciler`，源码中没有 apply 权限；Fake restart 使用共享 remote
state + 新 provider client 证明 apply count 始终为 1。该 worker 尚未注册任何 live provider，
Recovery Center 仍不提供不确定外部写入的 retry action。

### Wave C：v1.0 普通用户任务闭环

- Chat、Automation、Review、Recovery 使用同一 Agent/Tool/Connector 状态来源；
- 文件、Office、浏览器、连接器、Computer Use 都提供统一的执行中、等待、验证和恢复反馈；
- Subagent 只并行独立读取或边界明确的子任务，父 Agent 负责最终证据合成；
- Memory 继续采用可见选择理由、反馈和 review receipt，禁止连接器内容静默写入长期记忆。

### Wave D：更多 provider 与外部写入

- 新 provider 必须通过同一 contract suite，不能复制一套权限、凭据或恢复系统；
- Microsoft/Google 写入只有在 provider-specific idempotency、超时后 reconciliation、
  精确审批和重复副作用测试全部通过后才能 advertise capability；
- 真实账号验证、外部写入、发布、tag 和 push 都是独立决策，不由代码完成自动授权。

## 6. 迁移规则

1. 先建立新 Kernel 接口和兼容读取，再迁移调用方，最后删除旧路径。
2. 每个状态只能有一个写入者；跨 SQLite、保险库、文件系统或远端 provider 的操作必须有
   明确 intent、CAS identity、幂等结果和启动恢复。
3. 不做大爆炸式数据库重写；新增投影必须可从现有数据安全默认或显式进入 repair 状态。
4. 不把 provider 特例塞入共享 Tool Runtime；差异由 adapter 和 provider-specific validator 承担。
5. UI 只在 backend contract 稳定并有 secret/privacy 测试后接入。
6. 任一阶段全量测试失败，都不进入下一阶段，更不发布。

## 7. 强制验证门

- Rust 全量测试、前端测试/build、secret scan、release source check、`git diff --check` 全通过。
- 动态 marker 证明 token、code、verifier、provider error body、continuation 和绝对 vault 路径不泄漏。
- 崩溃点测试覆盖 SQLite intent 前后、保险库/文件原子替换前后、provider 成功但本地超时、
  generation 改变后的晚到提交。
- 并发测试覆盖 refresh/disconnect、sync/disconnect、download/disconnect、重复 wakeup、重复审批消费。
- 所有大小上限同时做 Content-Length 预检和流式实际字节限制。
- live provider 仍需新的明确用户许可；在此之前普通 UI 不提供可达入口。

## 8. v1.0 完成定义

v1.0 不是“界面上能点”的定义。完成必须同时满足：普通用户能发起并理解任务、自动任务可重启
恢复、连接器只授予最小能力、危险动作有精确审批、结果有证据、失败可解释且可恢复、敏感数据
不泄漏、DeepSeek 边界未被绕过，并且所有强制验证门通过。
