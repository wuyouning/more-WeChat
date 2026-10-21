# more-WeChat

微信启动与诊断工具 · macOS / Windows · v0.2.0 预览版

自动找到微信，设置目标数量，查看启动结果。已有实例会保留，只补齐不足的数量，登录操作在微信窗口中完成。

![桌面版界面预览，浏览器演示数据，不代表实机兼容结果](docs/images/desktop-preview.png)

## 当前兼容状态

**Windows 本机组合已通过双账号登录与消息收发验收。** 已验证范围为 Windows 11 x64（10.0.22631）与下表所列微信构建；其他系统组合及 macOS 仍显示未验证。“主进程已运行”本身不表示账号已登录。

| 平台 | 当前实现与验证 | 多开策略 |
| --- | --- | --- |
| Windows 10/11 x64 | **已验证：Windows 11 x64（10.0.22631）/ 微信 4.1.15.13，维护者确认两个不同账号同时登录且正常收发消息**；截图与通话待验 | EXE、DLL 指纹和系统/架构均匹配才显示已验证；同构建其他系统可尝试普通启动；其他微信构建暂仅支持单开与诊断 |
| macOS 13+ Apple Silicon / Intel | 原版身份、签名和权限预检查；纯逻辑测试通过，实机启动与双账号待验 | 给符合条件的微信 4.x 创建独立副本并本地签名 |

完整测试边界、构建指纹及验收步骤见 [兼容记录](docs/compatibility.md)。Windows 互斥锁适配只接受内置的具体构建和准确名称；当前没有启用远程句柄关闭规则，不会拿其他锁来试错。

## 安装与使用

在 GitHub **Actions → Desktop checks and preview bundles** 的成功运行中下载对应 artifact，解压后：

- Windows：运行 `more-WeChat_0.2.0_x64-setup.exe`，按当前用户安装。需要 WebView2；若系统未安装，安装器会下载该运行时，应用运行过程不联网。
- Mac：选择 `aarch64-apple-darwin`（Apple Silicon）或 `x86_64-apple-darwin`（Intel）的 DMG，将应用拖入 Applications。
- 当前 Windows 包未作发行者签名；Mac CI 包为 ad-hoc 签名，未公证。系统可能限制运行。正式公开分发前需要发行者签名与公证；应用不会替你绕过系统拦截。

打开后确认路径和版本，将目标总数量设为 1–5（默认 2），点击启动。未适配的 Windows 构建可以先设为 1。看到失败或部分成功时，查看错误说明，点击“重新检测”或“导出诊断信息”。重新检测得到的是当时的状态，不持续在后台扫描。

旧的 `.bat`、`.sh`、`.command` 文件现在是桌面应用入口。请将脚本一起下载；仅有 `.command` 时还需要同目录的 `.sh`。Mac 脚本可用 `bash more-wechat.sh` 运行；不再直接执行 2020 年的多开命令。

## 本地开发

需要 Node.js 22.12+、pnpm 10.28、Rust 1.93+。Windows 还需要 Visual Studio C++ Build Tools 和 WebView2；Mac 需要 Xcode Command Line Tools。具体依赖见 [Tauri 环境要求](https://v2.tauri.app/start/prerequisites/)。

```sh
pnpm install --frozen-lockfile
pnpm tauri dev
```

`pnpm dev` 只打开前端开发服务器，明确显示“界面预览 · 不会启动微信”，使用演示数据；真实系统接口只在桌面应用中运行。原生接口错误不会自动切换成演示成功。

```sh
pnpm test
pnpm test:browser
pnpm build
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# 在 Windows 上打包
pnpm tauri build --bundles nsis --ci
# 在 Mac 上打包（按架构添加 --target）
pnpm tauri build --bundles dmg --ci
```

安装包位于 `src-tauri/target/release/bundle/`。CI 为 Windows x64、Mac arm64 和 Mac x64 分别编译并上传预览包，不自动发布正式 Release。

Windows 原生集成检查：先运行 `pnpm dev`，再执行 `cargo build --manifest-path src-tauri/Cargo.toml` 和 `pnpm test:native`。测试只给自建调试应用启用本机 WebView2 调试端口，检查真实 IPC、无效输入、设置持久化、达标不新增、诊断导出及原生界面；不会操作微信登录界面。生产构建不启用该端口。

## 设置、数据与反馈

- 设置保存在系统应用数据目录下的 `io.github.wuyouning.more-wechat/settings.json`，包含微信路径和目标数量。
- Mac 受管理副本在 `~/Applications/more-WeChat/slot-N/WeChat.app`；原版升级后，仅在下次启动相应停止副本时刷新。稳定 Bundle ID 用于保留其容器身份，工具不迁移或删除聊天数据。详见 [Mac 实现与限制](docs/macos-adapter.md)。
- 工具不读取聊天内容、不自动登录、不上传日志、不自动提权、不结束现有微信。关闭工具后，微信继续运行。
- 诊断记录在内存中保留本次打开的最近 200 条，手动导出 JSON 才写盘。导出隐藏用户目录中的用户名；其他自定义路径仍可能包含个人信息，请检查后再分享。

遇到问题请使用 [启动问题模板](https://github.com/wuyouning/more-WeChat/issues/new?template=bug_report.yml)，附系统版本、微信版本、目标数量和检查过的诊断信息。

这个项目从 2020 年解决个人刚需的小脚本开始。图形版继续保持“小工具”的边界：首版做好启动与诊断，实例备注、窗口切换和自动更新留待后续。项目与腾讯无隶属关系。
