# OneSync 项目说明

本文档描述当前 Cargo 应用的代码结构、UI 组件、功能入口和核心逻辑，供人类维护者和 AI 助手快速理解项目。仓库中还包含 `docs/` 与 `OneDriveGUI/` 资源；当前 Rust 入口不直接引用这些文件，本次重构未删除或改写它们。

## 运行入口

- `src/main.rs`：声明应用模块并调用 `app::run()`。
- `src/app/mod.rs`：GTK/libadwaita 桌面应用入口，负责启动、装配和顶层 action 连接。

应用 ID 是 `io.github.onesync.Demo`，窗口标题是 `OneSync`，默认尺寸为 `1080x720`，最小尺寸为 `860x560`。

## 模块职责

- `account`：管理 OneSync profile 元数据，包括账号列表读写、新建 profile、认证文件路径和认证状态。
- `app`：GTK/libadwaita 应用层，包含状态、事件处理、布局、渲染、认证窗口、profile 管理窗口、确认窗口、通用 widget helper 和传输列表视图。
- `config`：解析、修改和写回 onedrive CLI 配置文件，保留注释、空行和未知行；写入前自动备份旧配置。
- `onedrive`：封装 onedrive CLI 调用，包括版本检测、认证、一次同步、持续同步、退出登录、进程停止和 CLI 输出解释。
- `settings`：读取 GUI 设置，当前支持自定义 onedrive CLI 路径。
- `transfer`：解析 onedrive 输出中的传输行，生成可供 UI 展示的 `SyncFile`。
- `utils`：集中处理配置根目录、home 目录展开和时间戳，避免账号与设置模块重复实现路径逻辑。

## UI 组件

- 主窗口：`adw::ApplicationWindow`，承载 `adw::ToastOverlay` 和 `adw::OverlaySplitView`。
- 侧边栏：`adw::ToolbarView` + `adw::HeaderBar` + `gtk::ListBox`，展示所有 profile；右上角 `list-add-symbolic` 按钮用于添加账号。
- Profile 行：`adw::ActionRow`，前缀头像图标，标题为 profile 名称，副标题为账号标识与状态。
- Profile 右键菜单：`gtk::Popover`，展示同步一次、开始持续同步、打开同步目录等占位动作；当前按钮为禁用态。
- 内容页头部：`adw::HeaderBar` + `adw::WindowTitle`，显示当前 profile 名称和账号标识。
- 设置按钮：头部右侧 `emblem-system-symbolic` 按钮；当前已连接空回调，作为后续设置页扩展点。
- 状态摘要：标题 `gtk::Label` 显示状态大类，详情 `gtk::Label` 显示配置目录、同步目录或错误信息。
- 操作按钮：一次同步、持续同步、更多账户操作。按钮内容由 `adw::ButtonContent` 统一设置。
- 账户操作 popover：当前包含编辑 Profile 按钮。
- 传输列表：`TransferList` 管理 `gtk::ListBox` 行、进度条动画、完成/进行中排序和同名传输更新。
- 添加账号窗口：输入名称、账号标识、同步目录，确认后创建 profile 并打开认证窗口。
- 认证窗口：生成认证链接、显示 auth URL、粘贴 redirect URI，并将回调写入 `auth-response`。
- 编辑 Profile 窗口：展示名称、账号标识、同步目录、认证状态；支持改名、退出登录、移除 profile。
- 移除确认窗口：要求精确输入当前 profile 名称后才允许移除。
- 通用确认窗口：用于退出登录等危险操作。
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
- stdout/stderr 被按换行和回车切分，完整输出用于错误分析，传输行交给 `transfer::parse_transfer_line()`。
- 同步结束后按成功、用户请求停止、认证失效、需要人工确认或错误更新 UI 状态。

### 持续同步

- 点击持续同步前会检查：已认证、CLI 可用、无一次同步运行。
- 后端执行 `onedrive --confdir <dir> --monitor --verbose`。
- 再次点击会发送停止请求，先尝试 Unix `TERM`，超时后调用 `kill()`。
- 监控进程停止后发送 `MonitorStopped`，UI 移除运行句柄并更新状态。

### 退出登录

- 编辑 Profile 中的退出登录会先弹出确认窗口。
- 确认后执行 `onedrive --confdir <dir> --logout`。
- 成功后状态变为 `NeedsAuth`，失败时保留错误信息。

## 传输解析逻辑

`transfer::parse_transfer_line()` 只识别明确表示文件传输或删除/移动/重命名的 onedrive 输出，忽略配置加载、扫描、普通状态等非传输行。

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

## 配置逻辑

- `OneDriveConfig::parse()` 按行解析配置，保留空行、注释、未知原始行和键值对。
- 多行配置值通过缩进延续，例如 `skip_file` 的多项值。
- `apply_edit()` 只改受支持字段，空字符串或 false 会移除对应可选键。
- `enable_transfer_metrics()` 强制设置 `display_transfer_metrics = "true"`。
- `write_with_backup()` 如果目标配置已存在，会先复制成 `config.bak-<timestamp>`。

## 后端事件

`onedrive` 模块通过 `std::sync::mpsc` 向 UI 发送事件：

- `ClientChecked`：CLI 版本检测结果。
- `AuthUrl`：认证 URL 已生成。
- `AuthFinished`：认证进程完成。
- `SyncFinished`：一次同步完成或停止。
- `LogoutFinished`：退出登录完成。
- `TransferEvent`：解析到单个文件传输状态。
- `MonitorStopped`：持续同步进程结束。

UI 每 250ms 在主线程轮询接收端，保证 GTK 更新发生在主线程。

## 错误和确认逻辑

- CLI 缺失、版本过低或版本无法解析时，`ClientCheck` 会生成可展示消息。
- onedrive 输出中包含登录、授权或 refresh token 失效关键字时，会触发重新认证流程。
- 网络、未知配置、上传下载失败、CLI 崩溃等输出会映射为中文可操作错误。
- 输出提示 `--resync`、big delete、download-only cleanup、upload-only + no-remote-delete 时，会弹出人工确认警告。

## 测试覆盖

当前单元测试覆盖：

- 重复同步目录校验、账号存储兼容未来字段。
- 配置多行值保留、空可选值移除、高级同步选项写回。
- onedrive 版本解析、错误映射、认证失效检测、确认状态检测、同步进程停止。
- 传输输出解析、进度解析、完成/失败/忽略非传输输出。
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
