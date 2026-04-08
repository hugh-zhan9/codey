# Local Customizations

这份文档记录当前仓库相对 upstream `openai/codex` 的本地定制功能，重点说明：

- 新增了什么能力
- 用户入口是什么
- 主要实现落在哪些文件
- 后续合并 upstream 时优先检查哪些冲突点

如果后面继续做本地增强，优先更新这份文档，再处理零散实现。

## 总览

当前本地定制主要分成 4 类：

1. 会话恢复体验增强
2. `/reload` 认证重载链路
3. 内置账号池、账号导入与自动切换
4. `codey` 打包与安装脚本

## 1. 会话恢复体验增强

### 功能

- `resume` 默认更偏向当前 project，而不是把所有 session 平铺出来
- 支持 `--all` 查看全量 session
- resume picker 按项目分组展示 session
- 当前项目优先显示，没有 cwd 的 session 放到 `No project`

### 用户入口

- `codex resume`
- `codex resume --last`
- `codex resume --all`
- TUI 内的 resume picker

### 主要实现

- `codex-rs/exec/src/lib.rs`
- `codex-rs/tui/src/lib.rs`
- `codex-rs/tui/src/resume_picker.rs`
- `codex-rs/tui_app_server/src/lib.rs`
- `codex-rs/tui_app_server/src/resume_picker.rs`
- `codex-rs/core/src/git_info.rs`
- `codex-rs/core/src/rollout/recorder.rs`

### 合并时重点

- 上游如果改了 resume 过滤逻辑、session lookup、rollout cwd 解析，这部分很容易冲突
- `resume_picker.rs` 是高频冲突文件，优先人工检查行为是否被回退

## 2. `/reload` 认证重载链路

### 功能

- 本地把旧的 `/reload auth` 收敛成 `/reload`
- `/reload` 用于手动重载当前认证状态
- 重载时会清理旧的额度快照和派生状态，避免 UI 继续显示旧账号信息

### 用户入口

- TUI slash command: `/reload`

### 主要实现

- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/app_event.rs`
- `codex-rs/tui/src/bottom_pane/chat_composer.rs`

### 合并时重点

- 上游如果改 slash command 分发、account RPC、chatwidget info message，这部分容易冲突
- 需要确认 `/reload` 仍然会触发账号状态、plan 和额度快照的完整刷新

## 3. 内置账号池

### 功能

- 内置保存多个账号快照
- 支持从 `codex-acc` 和 `cc-switch` 导入账号
- 支持手动切换当前账号
- 请求前检查当前账号是否仍可用
- 当前账号额度耗尽或认证失效时自动切到下一个健康账号
- 切号后走现有 reload 链路刷新当前会话状态
- 运行中命中额度耗尽并自动切号成功后，会自动重试上一条请求一次
- 自动切号后继承上一次请求的模型和关键会话配置
- 如果新账号不支持原模型或配置，直接报错，不自动降级
- status line 最前面显示当前账号别名，便于确认当前正在使用哪个账号

### 当前 slash command 入口

- `/import codex-acc [path]`
- `/import cc-switch [path]`
- `/switch`
- `/switch list`
- `/switch next`
- `/switch <alias>`

### 当前 `/switch` 展示内容

- 终端固定宽度框表，不再使用普通 info message 文本列表
- 当前账号标记
- `别名`
- `账号`
- `套餐`
- `5h额度`
- `周额度`
- `更新时间`

说明：

- `5h额度` / `周额度` 会展示“剩余百分比 + 恢复时间”
- `Quota` 仍然是后端内部判断字段，但不再作为 `/switch` 展示列
- `/switch` 表格本身不再依赖 TUI 的 bullet/info message 渲染，而是按纯文本行渲染，避免首行偏移

### 实现逻辑

#### 存储层

- 新增独立账号池文件，而不是复用 `auth.json`
- 当前设计使用 `~/.codex/account-pool.json`
- `auth.json` 仍然只表示当前活跃账号

#### 激活逻辑

- 激活某个账号时，把该账号的 auth 快照写回当前活跃认证文件
- 如果账号带有 config 快照，也一并恢复
- 同时更新池中的 `currentAlias`

#### 自动切换逻辑

- 当前账号不可用时，从账号池中选“下一个健康账号”
- 选择时会跳过：
  - `needs_relogin = true`
  - 明确 `quota_exhausted = true`
  - 在 cooldown 中的账号
- 在可选账号里，优先选择“额度恢复时间更早”的账号
  - 使用 5 小时额度和周额度窗口中的 `reset_at`
  - 取两个窗口里更早的恢复时间作为排序主键
- 切换成功后：
  - 落盘新的活跃 auth/config
  - reload 当前运行时认证状态
  - 校验当前线程继承下来的模型和配置是否仍可用
- 如果是运行中请求先因额度耗尽/认证失效失败，再触发的自动切号：
  - TUI 会保留最近一次真实用户提交的 `UserTurn`
  - 自动切号成功后，自动用原始模型和会话配置重放这次请求
  - 只自动重试一次，避免循环重试或重复发送

#### token 逻辑

- 不是每次请求都强制 refresh token
- 当前策略是“请求前轻量检查，接近过期才 refresh”
- 显式账号重载或切号场景可以强制 refresh
- 如果 refresh 失败且属于永久失效，账号会被标记为 `needs_relogin`

#### 额度逻辑

- 当前保存最近一次已知的 5 小时额度和周额度百分比
- 同时保存两个额度窗口的恢复时间
- `quota_exhausted` 的统一规则是：
  - `5h == 0%` 或 `week == 0%` 时为 `true`
- 提交前的自动切号检查不会每次都同步查额度接口
  - 优先使用账号池里最近一次缓存的额度状态
  - 当前实现使用短 TTL（60 秒）来避免每次回车都阻塞在额度查询网络请求上
  - 缓存过期或缺失时，才回退到实时额度查询
- `/switch` 现在会先全量刷新账号池，再展示列表
- 全量刷新已经改成“基于账号快照的限并发独立查询”，不再通过串行切换当前活跃账号来刷新
- 刷新时会给 TUI 一个开始提示，避免用户误以为命令没有执行
- 自动切号决策会基于额度状态、恢复时间和 token 健康状态进行

### 主要实现

#### `codex-login`

- `codex-rs/login/src/account_pool.rs`
- `codex-rs/login/src/account_pool_tests.rs`
- `codex-rs/login/src/lib.rs`

职责：

- 账号池存储
- 导入 `codex-acc` / `cc-switch`
- 激活账号
- 更新 token/usage health
- 选择下一个可切账号

#### `codex-app-server-protocol`

- `codex-rs/app-server-protocol/src/protocol/common.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`

新增 RPC：

- `accountPool/list`
- `accountPool/import`
- `accountPool/switch`
- `accountPool/switchNext`

#### `codex-app-server`

- `codex-rs/app-server/src/codex_message_processor.rs`

职责：

- 暴露账号池 RPC
- 请求前检查当前账号
- 必要时执行自动切号
- `/switch` 列表刷新时，并发刷新整池账号的 token/额度状态
- 切号后 reload auth/runtime
- 校验切号后模型和配置兼容性

#### `codex-tui`

- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/app_event.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/tests/slash_commands.rs`

职责：

- 暴露 `/import` 和 `/switch` 命令
- 展示账号池表格
- 手动切号
- 在自动切号后刷新当前会话状态
- 自动切号成功后自动重发上一条失败请求一次
- status line 最前面展示当前账号别名
- `/switch` 执行期间先显示刷新提示

### 合并时重点

- `codex-rs/app-server/src/codex_message_processor.rs` 是最大冲突热点
- `codex-rs/tui/src/app.rs` 和 `codex-rs/tui/src/chatwidget.rs` 也是高频冲突文件
- 如果上游改了：
  - account RPC 协议
  - slash command 分发
  - get account / auth status 刷新逻辑
  - rate limit 展示逻辑
  就必须重新核对这套账号池链路有没有断

### 当前限制

- 目前已经有后端能力和 slash command 入口
- 还没有做更完整的独立账号管理 UI
- token 与额度健康状态已经接上基础链路，但后续仍可以继续做缓存、后台低频刷新和更强的状态展示
- `/switch` 虽然已经改成限并发刷新，但仍是命令式列表展示，不是专门的交互式账号管理面板

## 4. `codey` 打包脚本

### 功能

- 提供一键把当前仓库打成 `codey` 包的脚本
- 支持直接全局安装
- 默认复用当前仓库版本号，也支持手动覆盖版本号

### 用户入口

- `bash scripts/package-codey-fast.sh`
- `bash scripts/package-codey-fast.sh --install`
- `bash scripts/package-codey-fast.sh --version <version> --install`

### 主要实现

- `scripts/package-codey-fast.sh`
- `scripts/package-codey.sh`

### 实现逻辑

- 构建 `codex-cli`
- 重新包装 npm tarball，包名改成 `codey`
- bin 入口映射到 `codey`
- 可选执行全局安装
- 脚本里已经处理过一次 `BUILD_ARGS` 空数组问题
- 也处理过旧打包进程导致的 target lock 问题

### 合并时重点

- 上游如果改 Node 打包结构、CLI 产物路径或安装脚本，这里要重新对齐

## 冲突热点

后续合并 upstream 时，优先人工检查这些文件：

- `codex-rs/app-server/src/codex_message_processor.rs`
- `codex-rs/tui/src/app.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/app_event.rs`
- `codex-rs/tui/src/resume_picker.rs`
- `codex-rs/exec/src/lib.rs`
- `codex-rs/login/src/account_pool.rs`
- `codex-rs/app-server-protocol/src/protocol/common.rs`
- `codex-rs/app-server-protocol/src/protocol/v2.rs`
- `scripts/package-codey-fast.sh`

## 建议的维护方式

后续本地定制继续增加时，按这套顺序维护：

1. 先更新这份文档的“功能说明”和“主要实现”
2. 再在 `docs/AI_CHANGELOG.md` 追加一次变更记录
3. 合并 upstream 时先对照本文件检查冲突热点
4. 合并完成后，重新核对用户入口是否仍可用

## 对照文档

- 变更流水：`docs/AI_CHANGELOG.md`
- 账号池设计：`docs/superpowers/specs/2026-04-07-account-pool-auto-switch-design.md`
