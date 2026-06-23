# OneSync Agent Guide

本文件是 OneSync 仓库的 Agent 上下文与工作规范。AI Agent 进入本仓库后，应先读取本文件，再根据任务阅读相关源码。

## Agent 工作规则

- 修改代码前先阅读相关模块，按当前实现理解行为，不凭文件名、旧说明或猜测做决定。
- 修改产品行为、架构边界、入口流程、状态规则、配置格式、测试命令或用户可见体验时，必须同步更新本文件。
- 如果只做内部重构但用户可见行为不变，应更新架构边界或模块职责中受影响的部分。
- 如果发现本文件与代码不一致，以代码为准，并在同一次修改中修正本文件。
- 不要把待办事项写成已实现行为；未完成能力必须明确标注为占位、禁用或扩展点。
- 保持 `AGENT.md` 为仓库级唯一项目/产品/Agent 说明；不要再新增独立的 `PROJECT.md`、`PRODUCT.md` 或同类重复说明文件。
- 本文件描述代码当前真实行为，不描述愿景、计划或已废弃实现。

## 必用 Skill

- 本仓库提供可随时查阅的项目本地 skill：`.agents/skills/onesync-onedrive-reference/SKILL.md`。
- 当需要确认上游 OneDriveGUI 或 abraunegg `onedrive` CLI 的行为时，优先使用该 skill，而不是凭记忆推断。
- 适用场景包括：多账号/profile 行为、认证流程、`--sync`、`--monitor`、`--dry-run`、`--confdir`、配置项、SharePoint/shared libraries、进程输出语义、安全确认和同步决策。
- 该 skill 是只读参考资料；除非用户明确要求更新 skill archive，不要修改 `.agents/skills/onesync-onedrive-reference/references/` 下的归档内容。
- 从 skill 得到的结论必须适配 OneSync 当前 Rust/GTK 架构，不要直接搬运上游 GUI 或 CLI 代码。
- 修改、评审或重构任何 Rust 代码时，必须使用 `rust-skills` 作为基础质量约束，重点检查 ownership/borrowing、错误处理、类型安全、测试覆盖和不必要 clone/分配。
- 修改 GTK4/libadwaita UI、窗口、widget、状态管理、主线程事件、后台线程回传或界面架构时，必须使用 `rust-gtk4-expert`。
- `rust-gtk4-expert` 的约束优先覆盖 UI 层实现：不得阻塞 GTK 主线程，GTK widget 只能在主线程操作，跨线程工作必须通过后端事件或 GLib 主线程调度回到 UI。
- 当 Rust 通用建议与 GTK/libadwaita 实践产生取舍时，以 Rust 安全性和 GTK 主线程正确性为硬约束，再按 OneSync 现有 `src/app/` 架构做最小一致改动。

## 产品定位

OneSync 是围绕 `onedrive` CLI 构建的 GTK/libadwaita Linux 桌面应用。它让用户通过图形界面管理一个或多个 Microsoft OneDrive 同步 profile，完成账号添加、认证、状态查看、一次同步、持续同步和账号维护等日常操作。

成功的产品状态是：用户无需阅读终端输出，也能完成常规 OneDrive 同步工作；当 `onedrive` CLI 需要认证、人工确认或出现错误时，OneSync 能把后端状态转译成清晰、可操作的界面反馈。

## 用户

目标用户是使用 Linux 桌面并通过 `onedrive` CLI 同步 Microsoft OneDrive 的用户。他们通常需要：

- 管理一个或多个 OneDrive 账号/profile。
- 完成 Microsoft 登录和回调认证。
- 检查当前账号是否已认证、是否正在同步、是否需要人工处理。
- 启动一次同步或持续同步。
- 查看正在传输、已完成或失败的文件操作。
- 在出现 CLI 错误、认证失效或危险同步提示时获得明确反馈。

## 产品体验原则

OneSync 的 UI 应当像一个可靠的系统工具：克制、直接、可预测。

- 把下一步需要执行的动作放在用户当前关注的位置。
- 每个对话框只突出一个主操作。
- 优先使用标准 GTK/libadwaita 控件和行为，不发明不必要的自定义交互。
- 工具界面保持紧凑、任务导向，避免营销式布局、装饰性面板、过大的 hero 视觉和含糊按钮。
- 保留用户输入的 profile 名称；只有明显默认值才由应用填充。
- 使用明确标签和动作化按钮，不只依赖颜色表达状态、危险或结果。
- 避免多个等价的关闭、取消、继续按钮处在同一视觉层级。

## 运行入口

- `src/main.rs`：声明应用模块并调用 `app::run()`。
- `src/app/mod.rs`：GTK/libadwaita 桌面应用入口，负责启动、装配和顶层 action 连接。

应用 ID 是 `io.github.onesync.Demo`，窗口标题是 `OneSync`，默认尺寸为 `1080x720`，最小尺寸为 `860x560`。

## 模块职责

- `profile`：定义和持久化 OneDrive profile，包括 profile 模型、状态、配置文件编辑、GUI 设置、认证文件路径和同步目录。`profile::edit::save_profile_edit` 是可独立测试的配置写入用例，从 GTK 编辑对话框中抽出 OneDriveConfig 读取/应用/备份写入和选择性同步列表写入。
- `operation`：定义 OneSync 的运行态语言，包括 `OperationKind`、`OperationPhase`、`AccountOperation`、控件状态派生和 operation registry。UI 与后端都使用这套类型表达“某个 profile 正在执行什么”。
- `event`：定义后台到 UI 的事实流和事件 payload。`FileChange`、`PreviewChange` 等文件变化数据属于 event payload，不是独立顶层架构概念。
- `adapter`：外部系统 adapter。`adapter::onedrive` 负责版本检测、认证、同步、预览、监控、reconcile、进程停止和 onedrive 输出解析；`adapter::graph` 负责账号身份查询和应用单项预览变更，`adapter::graph::http` 提供 `response_to_io` 共享转换。adapter 只通过 `BackendEvent` 向 UI 报告事实。
- `app`：GTK/libadwaita 应用层，负责把用户动作转换为 operation 请求，把 backend event 归约为 `AppState`，并从状态渲染 UI；GTK widget 只能在主线程操作。`app::events::reduce_outcome` 是纯函数，把 `OperationOutcome` 归约为 `OutcomeResolution`（含 `AccountStatus` 与可选 confirmation），不含 GTK，可独立测试。
- `utils`：集中处理配置根目录、home 目录展开、时间戳和路径拆分（`sync_path` 把相对路径拆为父目录与名称，供 single-directory scope 与 preview reconcile 共用），避免 profile 与 adapter 模块重复实现路径逻辑。

## UI 组件

- 主窗口：`adw::ApplicationWindow`，承载 `adw::ToastOverlay` 和 `adw::OverlaySplitView`。
- 侧边栏：`adw::ToolbarView` + `adw::HeaderBar` + `gtk::ListBox`，展示所有 profile；右上角 `list-add-symbolic` 按钮用于添加账号。
- Profile 行：`adw::ActionRow`，前缀头像图标，标题为 profile 名称，副标题为账号标识与状态。
- Profile 右键菜单：`gtk::Popover`，提供同步一次、开始持续同步、打开同步目录动作。
- 内容页头部：`adw::HeaderBar` + `adw::WindowTitle`，显示当前 profile 名称和账号标识。
- 设置按钮：头部右侧 `emblem-system-symbolic` 按钮；当前已连接空回调，作为后续设置页扩展点。
- 状态摘要：标题 `gtk::Label` 显示状态大类，详情 `gtk::Label` 显示配置目录、同步目录或错误信息。
- 操作按钮：一次同步、持续同步、更多账户操作。按钮内容由 `adw::ButtonContent` 统一设置。
- 账户操作 popover：当前包含编辑 Profile 按钮。
- 传输列表：`TransferList` 管理 `gtk::ListBox` 行、进度条动画、完成/进行中排序、同名传输更新，以及预览变更的应用/放弃按钮。
- 添加账号窗口：输入名称、账号标识、同步目录，确认后创建 profile 并打开认证窗口。
- 认证窗口：生成认证链接、显示 auth URL、粘贴 redirect URI，并将回调写入 `auth-response`。
- 编辑 Profile 窗口：使用 GNOME/libadwaita 风格的概览页，根分组标题为“账户设置”。每个 `ActionRow` 展示一个配置对象的摘要，并进入完整子页编辑；`账户信息` 合并 Profile 名称、账号标识、认证状态和同步目录，同步目录通过目录选择器修改，打开前会提示不会移动已有文件及潜在同步风险；`同步范围` 在子页中使用 tab 标签切换完整配置 section；`同步方向` 使用三个单选式 `ActionRow`，且只在“只上传到 OneDrive”时显示用于控制“保护云端文件”的 `Switch`；`自动同步` 使用对称的 `- 数字 +` 步进控件设置检查间隔和完整扫描频率，并在底部提供独立的“恢复默认”按钮。
- 移除确认窗口：要求精确输入当前 profile 名称后才允许移除。
- 通用确认窗口：用于大量删除等危险同步操作。
- 警告窗口：用于展示 onedrive 需要人工确认的状态，例如 `--resync`、大量删除或配置组合风险。
- Toast：所有短反馈通过 `adw::ToastOverlay` 展示。

## 功能流程

### 启动

1. `app::run()` 创建 `adw::Application`。
2. `build_ui()` 读取 GUI 设置和账号存储。
3. 构建侧边栏、内容区和全局状态 `AppState`。
4. 刷新磁盘认证状态，重建 profile 列表，刷新内容区。
5. 安装后端事件轮询器，并异步检测 onedrive CLI 版本。

### Profile 管理

- 账号数据存储在 `$XDG_CONFIG_HOME/onesync/accounts.json`，没有 `XDG_CONFIG_HOME` 时使用 `~/.config/onesync/accounts.json`。
- 新建 profile 会生成唯一 ID，创建配置目录，写入基础 `config`，并创建本地同步目录。
- 移除 profile 只从 OneSync 列表删除，不删除云端文件，也不删除本地同步目录。
- 改名只修改账号存储中的 profile 名称，不移动配置目录或同步目录。

### 状态模型

- `AccountStatus` 只表示持久 profile 状态：`NeedsAuth`、`Authenticated` 或 `Error(String)`。
- `AppState.operations` 是 UI 运行态的单一来源，按账号 ID 记录 `OperationKind` 和 `OperationPhase`。
- `OperationKind` 当前包括认证、一次同步、预览和持续同步；`OperationPhase` 当前包括运行中和停止中。
- `operation_handles` 只保存后端进程句柄，用于停止当前 CLI operation；渲染层不从句柄 map 派生 UI 状态。
- `operation::controls_for()` 根据同步模式、认证状态、CLI 可用状态和 `CommandRuntime` 纯函数派生同步/预览按钮模型。

### 认证

- 认证窗口启动 `onedrive --confdir <dir> --auth-files <auth-url>:<auth-response>`。
- 后端线程轮询 `auth-url` 文件，读到 URL 后发送 `BackendEvent::AuthUrl`。
- 用户粘贴 redirect URI 后，UI 写入 `auth-response` 文件。
- onedrive 进程结束后发送 `AuthFinished`，成功则状态变为 `Authenticated`，失败则写入错误状态。
- 同步或监控发现认证失效时，会设置 `NeedsAuth` 并重新打开认证窗口。

### 一次同步

- 点击一次同步前会检查：已选择账号、已认证、CLI 可用、无其他活动操作、持续同步未运行。
- 启动时会确保配置中 `display_transfer_metrics = "true"`，以便输出进度。
- 后端执行 `onedrive --confdir <dir> --sync --verbose`。
- stdout/stderr 被按换行和回车切分，完整输出用于错误分析，传输行交给 `adapter::onedrive::parse_file_change_line()`。
- 同步结束后按成功、用户请求停止、认证失效、需要人工确认或错误更新 UI 状态。

### 持续同步

- 点击持续同步前会检查：已认证、CLI 可用、无一次同步运行。
- 后端执行 `onedrive --confdir <dir> --monitor --verbose`。
- 再次点击会发送停止请求，先尝试 Unix `TERM`，超时后调用 `kill()`。
- 监控进程停止后发送 `MonitorStopped`，UI 移除运行句柄并更新状态。

### 预览与应用单项变更

- 手动模式下的预览按钮启动 `onedrive --confdir <dir> --sync --dry-run --verbose`。
- dry-run 输出中的传输行会解析为 `PreviewChange`，同一账号的预览变更保存在 `AppState.previews`。
- `PreviewChange` 只保存行为必要字段：ID、路径、可选源路径、传输类型、应用动作、意图和状态。
- 预览行可单项应用或放弃；应用时 UI 将该变更加入 `applying_preview_changes` 并调用 Graph API 执行上传、下载、删除、移动或重命名。
- Graph 应用完成后会触发局部 reconcile，用 onedrive CLI 更新同步状态；成功后从预览列表移除该变更，失败时保留行并展示可重试错误。

## 传输解析逻辑

`adapter::onedrive::parse_file_change_line()` 只识别明确表示文件传输或删除/移动/重命名的 onedrive 输出，忽略配置加载、扫描、普通状态等非传输行。

识别前缀包括：

- 下载：`Downloading file:`, `Downloading file`, `Downloading:`
- 上传：`Uploading:`, `Uploading new file:`, `Uploading new file`, `Uploading file:`, `Uploading file`
- 更新：`Uploading modified file:`, `Uploading modified file`
- 删除：`Deleting item from Microsoft OneDrive:`, `Deleting local file:`, `Deleting remote file:`, `Deleting file:`, `Deleting local item:`, `Deleting remote item:`, `Deleting item:`
- 移动：`Moving file:`
- 重命名：`Renaming file:`

状态规则：

- 输出包含 `failed!` 或 ` ... failed` 时，状态为 `<动作>失败`，进度为百分比或 `0.0`。
- 删除类操作没有 `done` 也视为完成。
- 输出以 `done.`、`done` 结尾，或包含 ` ... done` 时，状态为 `<动作>完成`，进度为 `1.0`。
- 输出包含百分比时，百分比转换为 `0.0..=1.0` 的进度。
- 其他已识别但未完成的行状态为 `正在<动作>`，默认进度为 `0.0`。

### onedrive CLI 输出样本

样本来源：

- 真实测试：2026-06-03 使用 `onedrive v2.5.10-1+np1+1.3`、OneSync profile `massey-1780217306`、同步目录 `~/Massey OneDrive`，在隔离目录 `onesync-agent-output-20260603-163850` 中执行 `--sync --verbose --single-directory`、`--download-file` 和 `--remove-directory`。
- 参考样本：`.agents/skills/onesync-onedrive-reference/references/upstream/OneDriveCLI/client-architecture.md` 与 `business-shared-items.md` 中保存的 abraunegg `onedrive` CLI 文档输出。

真实传输输出：

```text
Uploading new file: onesync-agent-output-20260603-163850/upload-new.txt ... done
Uploading modified file: onesync-agent-output-20260603-163850/upload-new.txt ... done
Uploading new file: onesync-agent-output-20260603-163850/download-source.txt ... done
Downloading file: onesync-agent-output-20260603-163850/download-source.txt ... done
Deleting item from Microsoft OneDrive: onesync-agent-output-20260603-163850/upload-new.txt
Deleting item from Microsoft OneDrive: onesync-agent-output-20260603-163850/download-source.txt
```

真实传输辅助输出，不应解析为文件传输：

```text
Processing: onesync-agent-output-20260603-163850/upload-new.txt
Local file time discrepancy detected: onesync-agent-output-20260603-163850/upload-new.txt
Transfer Metrics - File: onesync-agent-output-20260603-163850/upload-new.txt | Size: 69 Bytes | Duration: 0.99 Seconds | Speed: 0.00 Mbps (approx)
The requested directory to delete was found on OneDrive - attempting deletion
The requested directory to delete online has been deleted
Sync with Microsoft OneDrive is complete
```

上游文档中出现的下载输出变体：

```text
Downloading file ./1.txt ... done
Downloading file: ./file to share.docx.url ... done
Downloading file: my_shared_folder/my_folder/file_one.txt ... done
Downloading file: Files Shared With Me/test user (testuser@mynasau3.onmicrosoft.com)/file to share.docx ... done
Downloading file: Files Shared With Me/test user (testuser@mynasau3.onmicrosoft.com)/no_download_access.docx ... failed!
```

上游文档中出现的上传输出变体：

```text
Uploading new file ./1-onedrive-client-dev.txt ... done.
Uploading new file 1-onedrive-client-dev.txt ... done.
Uploading new file: onesync-agent-output-20260603-163850/upload-new.txt ... done
Uploading modified file: onesync-agent-output-20260603-163850/upload-new.txt ... done
```

OneSync 测试中保留的进度输出变体：

```text
Uploading: ./.onesync-progress-test/upload-progress.bin ... 37%  |  ETA    00:00:10
Uploading: ./.onesync-progress-test/upload-progress.bin ... 37.5%  |  ETA    00:00:10
Uploading: ./.onesync-progress-test/upload-progress.bin
```

模式匹配约束：

- 只把行首明确是传输动作的输出解析成 `SyncFile`；`Processing:`、`Transfer Metrics - File:`、状态总结、目录删除确认和扫描输出都必须忽略。
- `Downloading file` 需要同时兼容有冒号和无冒号形式：`Downloading file: path ... done`、`Downloading file ./1.txt ... done`。
- `Uploading new file` 和 `Uploading modified file` 需要同时兼容有冒号和无冒号形式，且 `done` 可能是 `done` 或 `done.`。
- `Deleting item from Microsoft OneDrive:` 是真实同步删除远端文件的输出，没有 `done` 后缀，应直接视为完成。
- `failed!` 和 ` ... failed` 应优先于 `done` 判断，避免把失败下载误判为完成。
- 带百分比的行应从 `%` 前向后解析最后一个数字片段，兼容整数和小数百分比。

## 配置逻辑

- `OneDriveConfig::parse()` 按行解析配置，保留空行、注释、未知原始行和键值对。
- 多行配置值通过缩进延续，例如 `skip_file` 的多项值。
- `apply_edit()` 只改受支持字段，空字符串或 false 会移除对应可选键。
- `enable_transfer_metrics()` 强制设置 `display_transfer_metrics = "true"`。
- `write_with_backup()` 如果目标配置已存在，会先复制成 `config.bak-<timestamp>`。

## 后端事件

`adapter` 模块通过 `std::sync::mpsc` 向 UI 发送 `event::BackendEvent`：

- `ClientChecked`：CLI 版本检测结果。
- `AuthUrl`：认证 URL 已生成。
- `AuthFinished`：认证进程完成。
- `SyncFinished`：一次同步完成或停止。
- `TransferEvent`：解析到单个文件传输状态。
- `PreviewEvent`：解析到单个 dry-run 预览变更。
- `PreviewFinished`：预览进程完成或停止。
- `PreviewApplyProgress`：Graph 应用单项预览变更的进度。
- `PreviewApplyFinished`：Graph 应用请求完成。
- `PreviewReconcileStarted`：应用后开始用 onedrive CLI 更新同步状态。
- `PreviewReconcileFinished`：应用后的同步状态更新完成。
- `ConfirmationRequired`：CLI 输出要求人工确认。
- `MonitorStopped`：持续同步进程结束。

UI 每 250ms 在主线程轮询接收端，保证 GTK 更新发生在主线程。

## 错误和确认逻辑

- CLI 缺失、版本过低或版本无法解析时，`ClientCheck` 会生成可展示消息。
- onedrive 输出中包含登录、授权或 refresh token 失效关键字时，会触发重新认证流程。
- 网络、未知配置、上传下载失败、CLI 崩溃等输出会映射为中文可操作错误。
- 输出提示 `--resync`、big delete、download-only cleanup、upload-only + no-remote-delete 时，会弹出人工确认警告。
- 当 onedrive 输出要求 `--resync` 等人工确认时，OneSync 不把 profile 标记为错误状态；它保留认证状态并展示确认窗口。`--resync` 确认窗口提供“执行 resync”按钮，确认后以 `onedrive --confdir <dir> --sync --verbose --resync` 启动一次同步。

## 测试覆盖

当前单元测试覆盖：

- 重复同步目录校验、账号存储兼容未来字段。
- 配置多行值保留、空可选值移除、高级同步选项写回。
- onedrive 版本解析、错误映射、认证失效检测、确认状态检测、同步进程停止。
- 传输输出解析、进度解析、完成/失败/忽略非传输输出、dry-run 预览变更解析。
- 预览变更 Graph 路径处理、应用进度事件和应用后 reconcile 事件。
- 同步/预览/持续同步按钮模型派生。
- 移除 profile 时的精确名称确认。

验证命令：

```bash
cargo fmt --all -- --check
cargo test
```

## 架构边界

- GTK/libadwaita 代码只放在 `src/app/`。
- 根层领域模块不依赖 GTK/libadwaita。
- `app` 调用 `account`、`config`、`settings`、`onedrive`、`transfer` 和 `utils`。
- `onedrive` 是 CLI adapter，内部拆分为事件类型、输出解析和进程管理。
- `transfer` 只负责把 onedrive 传输输出解析成结构化进度。
