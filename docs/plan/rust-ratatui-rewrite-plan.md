# Rust + Ratatui 重写计划

## 背景

这个仓库当前是新骨架，`docs/plan/` 为空，实际参考实现放在 `reference/`。`reference/` 是 Go 版 BaiduPCS-Go，README 标记当前版本线到 v4.0.1，核心能力集中在：

- `reference/internal/pcsconfig/`：配置路径、账号、Cookie、UA、代理、并发和下载参数。
- `reference/baidupcs/`：百度 PCS/Pan API 封装，包含目录列表、元信息、搜索、下载链接、上传、分享等。
- `reference/requester/downloader/`：支持 Range、多 worker、断点状态和下载进度。
- `reference/docs/`：旧 PCS 文件 API 和结构化数据 API 文档。

本次 Rust 重写的主目标不是简单复刻 CLI，而是做一个远程网盘优先的 TUI 文件管理器：打开即用、兼容 BaiduPCS-Go 配置、异步加载目录、支持内存预览、保留 Vim 键位。

## 必须对齐的现有约束

### 配置兼容

Go 版配置文件名是 `pcs_config.json`，默认查找顺序如下：

1. `BAIDUPCS_GO_CONFIG_DIR` 指向的目录。
2. 程序目录下已有的旧版 `pcs_config.json`。
3. Windows: `%APPDATA%\BaiduPCS-Go`。
4. 其他系统: `$HOME/.config/BaiduPCS-Go`。

主要配置字段需要读兼容：

- 账号：`baidu_active_uid`、`baidu_user_list`。
- 用户：`uid`、`name`、`bduss`、`ptoken`、`stoken`、`baiduid`、`sboxtkn`、`cookies`、`accesstoken`、`workdir`。
- 网络：`appid`、`user_agent`、`pcs_ua`、`pcs_addr`、`pan_ua`、`enable_https`、`proxy`、`proxy_hostnames`、`local_addrs`。
- 下载：`cache_size`、`max_parallel`、`max_download_load`、`max_download_rate`、`no_check`、`savedir`。
- 上传：`max_upload_parallel`、`max_upload_load`、`max_upload_rate`、`ignore_illegal`、`u_policy`、`fix_pcs_addr`。

建议策略：第一阶段只读兼容 Go 配置，不主动改写原配置。Rust TUI 自己的偏好、布局、缓存策略写到 sidecar，例如 `$CONFIG_DIR/baidupcsr/ui_config.toml`。等配置模型稳定后，再考虑对 `pcs_config.json` 做受控写入。

### API 与鉴权

参考实现有两条主要下载路径：

- PCS 直连：`/rest/2.0/pcs/file?method=download` 和 `stream/download`。
- Pan/locatedownload：通过 `locatedownload`、`fidlist`、签名、Cookie 和 Pan UA 获取 dlink。

配置中保存的是敏感 Cookie/Token。Rust 版必须把鉴权、Cookie jar、UA、代理、HTTPS 开关、域名选择封装在 `pcs_client` 层，TUI 不直接接触原始 token。

### 下载与预览

Go 版下载器围绕磁盘 writer 设计；Rust 版的 Preview 不能复用“先落盘再打开”的思路。Preview 应该是独立的数据流：

- 用 HTTP Range 拉取前 N 字节或必要片段。
- 写入内存 buffer，不写临时文件。
- 支持取消、超时、大小上限和 LRU 淘汰。
- 超过上限或类型不支持时展示元信息，不强行下载。

需要把三种动作做成明确契约：

- Preview：只读、内存、固定预算、可取消、默认不落盘。
- Open：用户显式同意后才允许交给外部程序；如外部程序要求文件路径，必须显示说明会产生临时文件。
- Download：用户显式触发的落盘动作，走下载任务系统。

视频预览选择严格方案：播放预览也不能把远程文件直接交给播放器，不能让播放器绕过应用缓存策略直连百度 dlink。后续视频播放必须走应用自有的 bounded streaming proxy，由 Rust 进程控制 Range 请求、内存 chunk、淘汰、取消和统计。MVP 可以先只显示视频 metadata 或缩略帧；但一旦支持“播放”，就必须满足这个严格语义。

## 推荐总体架构

建议做成 Rust workspace，核心逻辑和 TUI 解耦：

```text
crates/
  baidupcsr-cli/       # binary: 启动 TUI、少量诊断命令
  baidupcsr-config/    # 兼容 pcs_config.json，管理 sidecar 配置
  baidupcsr-pcs/       # 百度 PCS/Pan API、鉴权、签名、错误模型
  baidupcsr-core/      # 目录模型、任务模型、缓存、预览协调器
  baidupcsr-preview/   # preview adapters、内存预算、视频 bounded proxy
  baidupcsr-tui/       # ratatui 视图、键位、状态渲染
```

候选技术栈：

- TUI：`ratatui` + 默认 `crossterm` 后端。Ratatui 0.30 的官方文档建议普通应用直接用主 `ratatui` crate，只有需要更细依赖控制时再单独用 `ratatui-crossterm`。
- 异步：`tokio`，TUI 主循环和网络 worker 用 channel 通信。
- HTTP：`reqwest` 或 `hyper`。先选 `reqwest`，因为 cookie、proxy、timeout、range 请求更快落地。
- 配置：`serde` / `serde_json` / `toml`。
- 缓存：内存 LRU 为主，必要时 metadata-only 持久缓存单独评估。
- 文本预览：`syntect` 或轻量 fallback；二进制/大文件默认只显示 metadata。
- 图片预览：优先做能力探测和接口抽象，后续按终端支持 Kitty/Sixel/iTerm2 协议。
- 视频预览：最终目标是本地 loopback streaming proxy + 固定内存 chunk cache；播放器只能访问 `127.0.0.1` 上带随机 token 的临时 URL，不能直接访问真实 dlink。

## TUI 目标形态

首屏应是实际可用的文件管理器，而不是命令帮助页：

```text
┌ path / account / quota / network state ───────────────────────┐
│ 左: 位置/历史/收藏 │ 中: 当前目录列表 │ 右: Preview 面板          │
│                   │                  │ 文本/图片/元信息/错误     │
├────────────────────────────────────────────────────────────────┤
│ 后台任务: refresh / preview / download / upload / errors        │
└ 状态栏: mode, selected, cached age, key hints, diagnostics ─────┘
```

Preview 面板要同时展示内容和预算状态，例如 preview 类型、已拉取字节数、内存占用、缓存命中、是否被截断、是否超过播放/预览上限。这样用户能确认“预览没有落盘，也没有全量下载”。

Vim 键位作为一等能力：

- `j/k` 上下移动，`h/l` 返回/进入，`gg/G` 首尾，`Ctrl-u/Ctrl-d` 翻页。
- `/` 搜索当前目录，`n/N` 跳转匹配。
- `r` 刷新，`p` 触发/锁定预览，`q` 退出。
- `d` 下载，`u` 上传，`m` 移动/标记，`yy/p` 复制/粘贴可放到后续阶段。
- `:` 命令模式，用于 `cd`、`config doctor`、`reload-config`、`open` 等。

默认使用 Vim 键位，但 keymap 应写入 Rust sidecar 配置，后续允许用户切换 yazi/ranger 风格或自定义按键。

## 可行性验证与设计判定

这个设计不能只靠“看起来合理”判断。每个关键阶段都要能在本机真实环境里做一次可重复的 smoke，并留下可审计的结果。好设计的判定标准如下：

- 开箱即用：读取本机现有 BaiduPCS-Go 配置后，`doctor` 能识别配置路径、active user、workdir、UA、代理和 Cookie 存在性，不要求重新登录。
- 只读安全：Stage 2 到 Stage 6 的 live smoke 不修改 `pcs_config.json`，不创建远端文件，不删除远端文件。
- 模块边界可验证：配置、PCS 网络、目录缓存、Preview、TUI 能分别用 mock 测试；真实账号只用于最后一层 live smoke。
- UI 不被网络阻塞：在慢网络、目录刷新、Preview 请求进行中，仍能移动光标、返回目录、退出。
- Preview 语义可证明：Preview 路径不调用下载任务，不写普通文件，不创建隐藏临时文件；UI 能显示内存预算和已拉取字节。
- 视频严格性可证明：视频播放时播放器命令行只包含本地 loopback URL，不包含百度 dlink；chunk cache 不超过配置预算。
- 失败可恢复：Cookie 失效、代理错误、远端 403、Range 不支持、dlink 过期都能显示明确错误，并允许重试或回退 metadata。
- 复杂度可控：如果一个阶段无法用 tmux smoke 证明它的核心价值，就说明该阶段边界太大，需要拆小。

### tmux 手动验证协议

所有 live smoke 都放到独立 tmux session 中执行，避免污染当前 shell 状态，也方便捕获输出。建议 session 命名格式：

```bash
tmux new-session -d -s baidupcsr-live
tmux rename-window -t baidupcsr-live:0 smoke
tmux split-window -h -t baidupcsr-live:0
tmux split-window -v -t baidupcsr-live:0.1
```

Pane 职责：

- pane 0：运行构建、测试和 `doctor`。
- pane 1：运行 TUI 或只读 CLI smoke。
- pane 2：看日志、进程、监听端口和播放器命令行。

每次验证结束后，用 `tmux capture-pane -p -S -2000 -t baidupcsr-live:0.<pane>` 捕获关键输出，整理成 `docs/validation/YYYY-MM-DD-stage-N.md`。输出必须打码：UID 可以保留，用户名、Cookie、BDUSS、STOKEN、dlink、access token 必须隐藏。

### live smoke 梯度

Stage 2 完成后：

- `cargo run -- doctor --redact`：显示配置路径、账号数量、active UID、workdir、UA/代理/Cookie 是否存在。
- `cargo run -- doctor --redact --config-dir <dir>`：验证显式配置目录查找。
- 验证点：不打印敏感值，不写 `pcs_config.json`，损坏配置有清晰错误。

Stage 3 完成后：

- `cargo run -- doctor --redact --live-readonly`：执行只读 API smoke，例如 quota、当前 workdir meta、当前目录 list 前若干项。
- `cargo run -- ls --readonly --limit 20 <workdir>`：确认真实目录列表字段解析正确。
- 验证点：真实账号可用；网络错误能带 operation 和 remote code；请求可超时退出。

Stage 5 完成后：

- 在 tmux pane 1 启动 TUI，手动执行 `j/k/h/l`、刷新、搜索、进入目录、返回目录。
- pane 2 观察日志和进程状态。
- 验证点：加载时 UI 不冻结；错误能展示；退出后终端状态恢复。

Stage 6 完成后：

- 对文本、小图片、大文件分别触发 preview。
- 检查 UI 预算面板：已拉取字节、内存占用、是否截断、是否命中缓存。
- 检查工作区和系统临时目录没有 preview 文件。
- 验证点：快速移动光标不会显示错文件；preview 取消后不继续拉旧文件。

Stage 7 完成后：

- 启动视频 preview，pane 2 用 `ss -ltnp` 确认只监听 `127.0.0.1`。
- 用 `ps -eo pid,args` 检查播放器命令行，只能出现本地 loopback URL，不能出现百度 dlink。
- 快速 seek 和切换文件，观察 chunk cache 预算、session 失效和上游请求取消。
- 验证点：不落盘、不直连、不超预算；不满足严格语义时回退 metadata。

这个手动验证协议是设计的一部分。后续实现时，任何影响配置、网络、Preview、视频播放的 PR，都必须说明它通过了哪一级 tmux smoke，或者明确为什么该阶段还不能做 live smoke。

## 分阶段计划

### Stage 0: 基线与兼容测试资产

目标：把 Go 参考实现中必须兼容的契约固定下来。

任务：

- 收集 `pcs_config.json` 最小样例、完整样例、老版本样例和异常样例。
- 记录默认配置路径查找规则，覆盖 `BAIDUPCS_GO_CONFIG_DIR`。
- 梳理文件模型字段：`fs_id`、`app_id`、`path`、`server_filename`、`ctime`、`mtime`、`md5`、`block_list`、`size`、`isdir`、`ifhassubdir`。
- 梳理下载链路：PCS download、stream download、locatedownload、Pan API dlink。
- 固化 Preview/Open/Download 三种动作的边界，测试中禁止 Preview 写入文件。
- 准备视频样例分类：普通 MP4、需要尾部 moov 的 MP4、大文件视频、不支持 Range 的响应、远端 403/重定向响应。
- 建立 mock API fixtures，避免早期开发依赖真实账号。
- 建立 `docs/validation/` 和 smoke 记录模板，包含环境、命令、输出摘要、敏感信息打码说明、通过/失败结论。

验收：

- 有配置 fixture 和 API fixture。
- 能明确列出 MVP 需要兼容的字段和暂不支持的字段。
- 不触碰真实用户配置写入。

### Stage 1: Rust Workspace 与基础工程

目标：先搭一个能持续演进的 Rust 工程，不让 TUI、网络和配置混在一起。

任务：

- 初始化 workspace 和五个 crate：CLI、config、PCS client、core、TUI。
- 配置 `clippy`、`rustfmt`、基础 CI 命令。
- 建立统一错误类型，区分配置错误、鉴权错误、远端 API 错误、网络错误、取消、超时。
- CLI 先提供两个入口：默认启动 TUI，`doctor` 输出配置查找和账号状态。
- `doctor` 从第一版开始支持 `--redact`，后续 live smoke 输出默认走打码路径。

验收：

- `cargo test` 和 `cargo clippy` 可运行。
- 没有真实网络请求也能启动空 TUI shell。
- 模块边界清楚，TUI 不依赖 API 细节。

### Stage 2: BaiduPCS-Go 配置读兼容

目标：实现开箱即用，复用 Go 版登录状态。

任务：

- 实现配置目录查找，行为对齐 Go 版。
- 用 `serde` 读取 `pcs_config.json`，保留未知字段。
- 选择 `baidu_active_uid` 对应账号，构造只读账号上下文。
- 支持从 `cookies` 中读取 `BDUSS/STOKEN`，也支持单独字段。
- sidecar 配置只保存 Rust TUI 偏好，不污染 Go 配置。
- `doctor` 显示：配置路径、账号数量、当前账号、workdir、代理、UA 是否存在；敏感字段只显示是否存在，不打印值。
- 保存 Rust sidecar 配置：keymap、布局、preview 内存预算、视频 chunk 预算、最近路径、收藏路径、是否启用实验性视频播放。

验收：

- 能读取真实 Go 版配置并进入账号 workdir。
- 空配置、损坏 JSON、无 active user 都有明确错误。
- 不把 token 打到日志或 UI。
- 通过 Stage 2 tmux smoke，并在 `docs/validation/` 留下打码记录。

### Stage 3: PCS/Pan 网络核心

目标：先做只读远程文件浏览需要的 API。

任务：

- 实现 HTTP client builder：Cookie jar、UA、Pan UA、PCS UA、proxy、proxy_hostnames、enable_https、timeout。
- 实现目录列表、单文件 meta、批量 meta、搜索。
- Port locatedownload 所需签名：`devuid`、`time/rand/cuid` 参数。
- 实现 download URL 选择策略，跳过明显不可用的候选链接。
- 建立远端错误模型，保留 remote error code 和 operation。
- 用 mock server 测试 URL、query、header、cookie 和错误解析。

验收：

- 能列出目录并解析完整文件模型。
- 能拿到普通文件的 preview/download URL。
- 网络层可取消，超时可配置，错误能被 TUI 显示为可读信息。
- 通过 Stage 3 tmux live-readonly smoke，不产生远端写操作。

### Stage 4: 异步目录缓存与无感刷新

目标：达到“切目录立即响应，后台刷新再更新”的体验。

核心策略：stale-while-revalidate。

任务：

- 每个远程目录维护一个 `DirectoryEntryCache`：路径、排序、条目、加载状态、最后刷新时间、错误。
- 进入目录时先显示缓存；同时发起后台刷新。
- 当前目录静止时预取选中目录、兄弟目录或历史路径。
- 切换目录时取消不再需要的 preview 请求，但不取消可复用的目录刷新。
- 每次刷新返回 generation id，旧响应不能覆盖新状态。
- 文件操作成功后定向失效相关目录。
- 后台刷新发现差异时记录变更：新增、删除、mtime 变化、size 变化，并在 UI 中短暂标记。
- 维护 session 状态：上次账号、上次路径、最近路径、收藏路径、搜索历史。

验收：

- 网络慢时 UI 仍可移动、返回、搜索缓存。
- 同一路径并发刷新会合并或丢弃旧结果。
- 进入目录时一定会触发一次新鲜度检查。
- 后台刷新后用户能看出目录发生了哪些变化。

### Stage 5: Ratatui TUI MVP

目标：得到可长期使用的远程文件浏览器骨架。

任务：

- 实现应用状态机：Normal、Search、Command、Confirm、TaskFocus。
- 实现三栏布局：位置/历史、文件列表、Preview。
- 文件列表支持目录优先、排序、选中态、加载态、错误态、空目录态。
- 实现 Vim 键位和基础命令模式。
- 读取 sidecar keymap，默认 Vim 键位，可覆盖单键和组合键。
- 状态栏展示账号、路径、缓存年龄、后台任务数量、网络错误摘要。
- Preview 面板展示预算状态：已拉取字节数、内存占用、缓存命中、截断原因、是否落盘。
- 增加诊断面板入口，展示 `doctor` 的配置、账号、代理、UA、API smoke 结果。
- 所有网络结果通过 event channel 回到 UI，渲染层不 await 网络。

验收：

- 可以用键盘浏览远程目录。
- UI 在加载、失败、取消、刷新时状态清楚。
- 长文件名、中文路径、小终端宽度不破坏布局。
- 用户能在 UI 里确认 preview 使用的是内存预算，不是隐式下载。
- 通过 Stage 5 tmux TUI smoke，退出后终端状态恢复。

### Stage 6: 内存 Preview 基础

目标：实现类似 yazi 的右侧预览，但针对远程文件和网盘 API 做内存安全约束。

任务：

- 实现 `PreviewManager`：按账号、`fs_id`、`mtime`、`size` 建 key，统一管理请求取消、LRU、预算和错误。
- 实现 `PreviewAdapter` 接口：输入远程文件元信息和 bounded byte reader，输出可渲染 preview model。
- 文件类型探测：扩展名、MIME、magic bytes、大小。
- 文本预览：Range 拉取前若干字节，UTF-8/GBK 等编码探测，行数和字节数上限。
- 图片预览：先做 metadata 和终端能力探测；后续按 Kitty/Sixel/iTerm2 逐步实现，且必须进入同一预算系统。
- PDF/Office/压缩包/视频：基础阶段只显示 metadata；后续可做第一页/目录/媒体信息，但仍不得默认落盘。
- Preview 请求绑定当前选中项，选择变化时取消旧请求。
- Preview cache 使用内存 LRU，按账号+fs_id+mtime+size 建 key。
- 默认单文件 preview 上限建议 16 MiB，可配置；超过上限只显示摘要和“显式下载”动作。
- 对所有 preview 写路径做测试拦截：Preview 代码不能创建临时文件，不能调用普通下载任务。

验收：

- 文本文件可无落盘预览。
- 大文件不会把内存打爆。
- 快速移动光标不会显示错文件内容。
- 不创建临时预览文件。
- Preview/Open/Download 三种动作在代码路径和 UI 文案上可区分。
- 通过 Stage 6 tmux preview smoke，确认 preview 不落盘、不继续拉旧文件。

### Stage 7: 严格视频预览

目标：实现用户选择的 B 方案：视频预览严格不落盘，严格固定内存缓存，播放链路由应用控制。

设计约束：

- 播放器不能直接拿百度 dlink，必须只访问本机 `127.0.0.1` 上的临时 URL。
- 本地 streaming proxy 必须验证随机 token，绑定单个 preview session，退出或切换文件时立即失效。
- Proxy 对上游百度链接发 Range 请求，只保留固定大小的内存 chunk/ring buffer。
- 禁止 disk cache。若外部播放器无法关闭磁盘缓存或无法只走本地 proxy，则禁用该播放后端。
- 固定预算分成三块：普通 preview LRU、视频 chunk cache、播放器进程允许的缓存参数。应用能严格控制前两块；播放器内部解码缓冲只能通过后端参数约束，不能假装完全可控。
- 初期只承诺 progressive MP4 这类可 Range 播放格式；对容器结构不适合远程按需播放的文件，回退到 metadata/缩略帧。

任务：

- 实现 `VideoPreviewSession`：文件身份、当前播放状态、上游 dlink、下游 token、内存预算、取消句柄。
- 实现本地 loopback HTTP server：仅监听 `127.0.0.1`，生成一次性 URL，支持 `Range`、`HEAD`、`Content-Range`、`Accept-Ranges`。
- 实现 chunk cache：固定 chunk 大小、固定最大 chunk 数、LRU 淘汰、顺序预读、seek 后丢弃无关窗口。
- 实现上游 Range fetcher：复用 PCS/Pan Cookie、UA、代理和重定向策略；遇到 403、链接过期或 Range 不支持时返回可读错误。
- 集成受控播放器后端：优先选择能从本地 HTTP URL 播放且可关闭磁盘缓存的后端；启动参数必须关闭 disk cache，并限制内存 cache。
- TUI 显示视频预览状态：当前 offset、buffer window、已拉取字节、chunk 命中率、内存预算、是否可 seek、错误。
- 加入 kill switch：sidecar 配置里默认可先关闭视频播放，只保留 metadata；打开后仍必须走严格 proxy。

验收：

- 播放视频时工作区和系统临时目录不会出现 preview 文件。
- 播放器进程命令行不包含百度 dlink，只包含本地 loopback URL。
- 内存 chunk cache 不超过配置预算。
- 快速 seek 或切换文件会取消旧 session，不继续后台拉取旧视频。
- 不支持严格语义的终端或播放器后端只显示 metadata，不降级为直连 dlink。
- 通过 Stage 7 tmux video smoke，记录 `ss`、`ps` 和预算面板证据。

### Stage 8: 任务系统与真实传输

目标：把下载、上传、复制、移动等耗时操作从 UI 导航中剥离。

任务：

- 统一 `TaskManager`：任务 id、状态、进度、速度、错误、取消、重试。
- 下载任务复用 Stage 3 的 dlink，但写入磁盘并支持 Range/断点续传。
- 上传任务对齐 Go 版策略：`skip`、`overwrite`、`rsync`。
- TUI 底部任务栏和任务详情面板。
- 操作完成后刷新相关目录缓存。

验收：

- 下载不会阻塞目录浏览。
- 任务可取消，可查看失败原因。
- 预览和下载是两条路径：预览默认不落盘，下载明确落盘。

### Stage 9: 高级文件管理能力

目标：补齐日常网盘操作。

任务：

- 新建目录、删除、重命名、移动、复制。
- 多选和批量操作。
- 搜索当前目录和递归搜索。
- 回收站、分享、转存、离线下载按使用频率逐步迁移。
- 对破坏性操作提供确认框和 dry-run 摘要。

验收：

- 常用文件管理操作能在 TUI 内完成。
- 破坏性操作不因误按单键直接执行。
- 操作后目录缓存一致。

### Stage 10: 打磨、发布与迁移

目标：让它变成可替代日常使用的工具。

任务：

- 跨平台终端测试：Linux/macOS/Windows，常见终端和 SSH 场景。
- 性能测试：大目录、慢网络、快速导航、大量 preview。
- 安全审计：token 日志泄漏、panic 输出、配置文件权限。
- 打包：release profile、静态链接可行性、shell completion、man page。
- 写迁移文档：如何复用 Go 配置、如何回退、哪些 Go 功能暂未迁移。

验收：

- 普通用户无需重新登录即可启动。
- 出错时能给出明确修复建议。
- 文档说明 Rust 版和 Go 版共存边界。

## 我建议增加的设计点

1. 配置写入分层：Go 配置只读兼容，Rust TUI 偏好写 sidecar，避免早期 bug 损坏登录态。
2. Preview 插件化：文本、图片、PDF、媒体、压缩包分别是 adapter，核心只管拉取 bytes、取消、缓存、预算。
3. 目录缓存采用 stale-while-revalidate：用户先看到旧数据，再无感刷新，而不是每次切目录都白屏等待。
4. 所有后台请求带 generation id：异步返回乱序时不会污染当前 UI。
5. 网络层与 TUI 完全解耦：未来可以加 headless CLI、daemon 或 GUI，不必重写百度 API。
6. 做 `doctor` 命令：专门排查配置路径、账号、代理、UA、API 连通性，能减少后续调试成本。
7. 默认保守传输：Preview 不落盘、不全量拉大文件；真实下载必须是显式动作。
8. 做 Preview 预算面板：把内存预算、命中、淘汰、截断、已拉取字节显示出来，避免用户误以为在后台全量下载。
9. 做路径工作区：记录最近路径、收藏路径、上次会话、搜索历史，让远程网盘浏览更像文件管理器而不是一次性命令。
10. 做可配置 keymap：默认 Vim，但用户可以切成 yazi/ranger 风格或自定义。
11. 视频播放坚持严格 proxy：先允许 metadata/缩略帧，真正播放必须由本地 bounded streaming proxy 控制，不能临时采用直连 dlink 方案。

## 风险清单

- 百度网盘接口不是稳定公开 API，locatedownload 和 Pan 签名可能变化，需要 mock + live smoke 双层验证。
- Cookie/Token 很敏感，日志、panic、debug 面板都必须打码。
- 图片/PDF 预览在终端能力上差异很大，MVP 不应该被图片协议绑死。
- 视频容器和播放器行为差异很大。应用能严格控制上游拉取和自己的 chunk cache，但播放器内部解码缓冲只能通过后端参数约束；文档和 UI 不能把这部分说成完全可控。
- 某些视频不适合远程 Range 播放，例如 moov 在尾部、索引缺失、远端不支持 Range、dlink 频繁过期。必须回退 metadata，而不是改成落盘。
- 大目录和快速移动会制造大量请求，必须做合并、取消和速率限制。
- 与 Go 版共用配置时，写兼容风险高，所以前期不改写原配置。

## MVP 边界

第一版建议只承诺：

- 读取 BaiduPCS-Go 配置并复用当前账号。
- TUI 浏览目录，支持 Vim 键位。
- 异步加载目录，缓存并后台刷新。
- 文本文件内存预览。
- 视频文件显示 metadata；播放能力必须等严格 proxy 成熟后再开启。
- 显式下载单文件。
- `doctor` 诊断命令。

暂不承诺：

- 完整上传。
- 分享/转存/离线下载。
- 所有图片协议。
- 视频直连播放器或落盘式视频预览。
- Office/PDF 高保真预览。
- 对 `pcs_config.json` 的写入迁移。
