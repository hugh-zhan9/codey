## [2026-03-20 16:01] [Feature]
- **Change**: 新增 /reload auth 命令以手动重载认证状态并清理旧额度缓存
- **Risk Analysis**: 主要风险在于认证重载后若仍有依赖旧认证的派生状态未同步，可能出现局部 UI 状态与真实账号不一致；本次已在命令内清空额度快照、重置 plan_type 并重新启动额度预取，风险中低。
- **Risk Level**: S2（中级: 局部功能异常、可绕过但影响效率）
- **Changed Files**:
- `codex-rs/tui/src/slash_command.rs`
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/tests.rs`
- `codex-rs/tui/src/bottom_pane/chat_composer.rs`
- `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__slash_reload_auth_info_message.snap`
----------------------------------------
## [2026-03-23 14:50] [Bugfix]
- **Change**: 调整 session 恢复默认可见性：取消 exec --last、tui 和 tui_app_server 恢复入口对当前 cwd 与默认 provider 的隐式过滤，确保切换配置后仍能找到已有 session，并补充回归测试。
- **Risk Analysis**: 主要风险在于恢复列表现在会跨项目、跨 provider 展示更多 session，误恢复到非当前上下文会更容易发生；本次只修改默认过滤，不改 rollout 存储与 archive 语义，并通过 exec、tui、tui_app_server 三个 crate 的测试验证行为。
- **Risk Level**: S2（中级: 局部功能异常、可绕过但影响效率）
- **Changed Files**:
- `codex-rs/exec/src/lib.rs`
- `codex-rs/tui/src/resume_picker.rs`
- `codex-rs/tui_app_server/src/resume_picker.rs`
----------------------------------------
## [2026-03-23 15:08] [Feature]
- **Change**: 为 resume picker 增加按项目分组展示：按 session 的 cwd 分组，当前项目优先，缺少 cwd 的会话归入 No project，并同步更新 tui 与 tui_app_server 的渲染、滚动与快照测试。
- **Risk Analysis**: 主要风险在于项目标题行会改变列表行数与滚动定位，如果显示行和真实 session 行映射不一致会导致选中项错位；本次通过新增分组排序单测、更新快照，并在两个 crate 的完整测试中验证，之后再执行 clippy fix 与 fmt。
- **Risk Level**: S2（中级: 局部功能异常、可绕过但影响效率）
- **Changed Files**:
- `codex-rs/tui/src/resume_picker.rs`
- `codex-rs/tui/src/snapshots/codex_tui__resume_picker__tests__resume_picker_table.snap`
- `codex-rs/tui/src/snapshots/codex_tui__resume_picker__tests__resume_picker_thread_names.snap`
- `codex-rs/tui_app_server/src/resume_picker.rs`
- `codex-rs/tui_app_server/src/snapshots/codex_tui_app_server__resume_picker__tests__resume_picker_table.snap`
- `codex-rs/tui_app_server/src/snapshots/codex_tui_app_server__resume_picker__tests__resume_picker_thread_names.snap`
----------------------------------------
## [2026-03-23 16:43] [Bugfix]
- **Change**: 拉取 origin/main 后合并本地改动，并解决 tui resume_picker 的 stash 冲突
- **Risk Analysis**: 本次只处理了一处冲突，核心是保留按项目全局可见的 session 列表行为，同时兼容上游接口调用；风险在于恢复列表的 provider 过滤被刻意放开，需要依赖既有测试覆盖避免回退。
- **Risk Level**: S2（中级: 局部功能异常、可绕过但影响效率）
- **Changed Files**:
- `codex-rs/tui/src/resume_picker.rs`
----------------------------------------
## [2026-03-23 17:16] [Feature]
- **Change**: 恢复 session 默认按当前 project 过滤，新增 --all 查看全量 session，并同步切换仓库 origin 到 codey.git
- **Risk Analysis**: 本次把 resume 的过滤语义从按 provider/全量平铺调整为按当前 project 默认过滤，风险主要在于 app-server 和本地 rollout 两条恢复路径必须保持一致；另外当前工作区存在与本次无关的 tui 编译阻塞，导致 tui 相关测试无法完整复核。
- **Risk Level**: S2（中级: 局部功能异常、可绕过但影响效率）
- **Changed Files**:
- `codex-rs/core/src/git_info.rs`
- `codex-rs/core/src/rollout/recorder.rs`
- `codex-rs/exec/src/lib.rs`
- `codex-rs/tui/src/lib.rs`
- `codex-rs/tui/src/resume_picker.rs`
- `codex-rs/tui_app_server/src/lib.rs`
- `codex-rs/tui_app_server/src/resume_picker.rs`
----------------------------------------
## [2026-03-23 17:29] [Bugfix]
- **Change**: 修复 chatwidget 中 /reload 命令的错误调用，并将旧的 /reload auth 入口收敛为 /reload
- **Risk Analysis**: 本次修复点集中在 tui 命令处理层，风险主要是旧的 /reload auth 调用路径现在会提示 Usage: /reload；如果有外部文档或习惯依赖旧写法，需要同步更新。
- **Risk Level**: S2（中级: 局部功能异常、可绕过但影响效率）
- **Changed Files**:
- `codex-rs/tui/src/chatwidget.rs`
- `codex-rs/tui/src/chatwidget/tests.rs`
- `codex-rs/tui/src/bottom_pane/chat_composer.rs`
----------------------------------------
## [2026-03-23 17:35] [Refactor]
- **Change**: 统一 /reload 相关测试与快照命名，并整理 tui 命令改动的索引状态
- **Risk Analysis**: 本次主要是命名和索引收尾，行为风险很低；唯一需要注意的是旧快照文件名已改，如果外部脚本硬编码引用旧名字会失效。
- **Risk Level**: S2（中级: 局部功能异常、可绕过但影响效率）
- **Changed Files**:
- `codex-rs/tui/src/chatwidget/tests.rs`
- `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__slash_reload_info_message.snap`
- `codex-rs/tui/src/bottom_pane/chat_composer.rs`
----------------------------------------
