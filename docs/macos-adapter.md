# macOS 独立副本预览适配

2026-10-22，v0.2.1 在 Intel Mac、macOS 26.3.1 (a)（25D771280a）、微信 4.1.13 上完成四开场景的窗口、登录与消息收发人工验收。Apple Silicon、其他系统及微信构建仍待实机验证，各组合应分别记录结果。`canMultiLaunch` 只表示 4.x、原版身份、Apple 签名及权限预检查通过，界面目前仍显示通用“版本未验证”；运行时尚未添加与本次人工记录对应的具体构建匹配规则。

2026-10-22，本机 Intel 启动器包已完成 Developer ID 签名、Apple 公证和票据装订，`.app`、`.dmg` 均通过 Gatekeeper 检查。此项属于启动器分发验证；微信多账号验收结果见 [兼容记录](compatibility.md#macos-状态)。当前 CI 仍生成 ad-hoc 预览包。

## 启动器首次打开与签名排查

先确认提示出现在哪一步：

| 阶段 | 含义与处理 |
| --- | --- |
| 打开 more-WeChat 时，系统要求到“隐私与安全”确认 | 当前 CI 预览安装包采用 ad-hoc 签名、未公证。确认来自本仓库后，可在“系统设置 → 隐私与安全”的安全性区域，对此应用选择“仍要打开”，按系统提示确认。它是当前应用的首次运行例外，不是辅助功能、完全磁盘访问或微信的摄像头授权。 |
| 工具内检测微信时报“微信原版签名验证失败” | 微信安装的完整性或腾讯身份检查失败。先使用修复后的启动器重新检测；旧版存在 `codesign -R` 表达式缺少 `=` 前缀的问题，会把合法微信误报为签名失败，进而显示未找到微信。仍失败时保留诊断原文，检查腾讯原版安装。 |
| 点击启动、生成副本时提示本地签名或副本签名失败 | 属于副本生成阶段，需检查具体错误；生成失败时保留旧副本。它与启动器是否已公证是不同检查。 |

`codesign --verify` 通过只表示签名完整性通过；ad-hoc 签名不会因此成为 Apple 信任的发行者签名。要使分发的启动器通过正常的 Gatekeeper 检查，需要 **Developer ID Application 签名及 Apple 公证**。Apple Development 证书不能代替此分发流程。

正式构建时，在构建环境安全配置 `APPLE_SIGNING_IDENTITY`（已安装到钥匙串的 Developer ID Application 身份），并配置 `APPLE_ID`、`APPLE_PASSWORD`（App 专用密码）、`APPLE_TEAM_ID`，或改用 Tauri 支持的 App Store Connect API 凭据。凭据不写入源码。当前 CI 显式使用 `APPLE_SIGNING_IDENTITY: '-'`，仍然只产出未公证预览包。

```sh
# 已在构建环境配置正式签名、公证凭据后，在 Intel Mac 上执行
pnpm tauri build --target x86_64-apple-darwin --bundles dmg --ci
# 检查 .app 的完整性和系统分发评估；路径按实际产物替换
codesign --verify --deep --strict "path/to/more-WeChat.app"
spctl --assess --type execute --verbose "path/to/more-WeChat.app"
xcrun stapler validate "path/to/more-WeChat.app"
```

不通过关闭 Gatekeeper、SIP 或全局清除隔离属性来处理此问题。参考 [Apple 首次打开说明](https://support.apple.com/guide/mac-help/open-a-mac-app-from-an-unknown-developer-mh40616/mac) 和 [Tauri macOS 签名、公证说明](https://v2.tauri.app/distribute/sign/macos/)。

### 已有付费账号，首次创建 Developer ID 证书

1. 使用 Account Holder 身份进入 [Apple Developer 的 Certificates](https://developer.apple.com/account/resources/certificates/list)，新增 **Developer ID Application** 证书。此项目分发 `.app` / `.dmg`，不选择 Developer ID Installer 或 Apple Development。
2. 上传本机准备的 `~/Library/Application Support/more-WeChat-signing/more-WeChat.certSigningRequest`，继续后下载 `.cer`。对应私钥 `developer-id.key` 保存在同一仓库外目录，目录权限 700、文件权限 600。CSR 可以提交给 Apple；私钥留在本机，不上传到仓库或聊天。
3. 下载 `.cer` 后，需要将本机生成的私钥及对应证书一起导入登录钥匙串。因为此 CSR 是通过 OpenSSL 生成，不能仅双击 `.cer` 就假定已安装可签名身份。导入后执行 `security find-identity -v -p codesigning`，必须能看到 `Developer ID Application: … (TEAMID)`。
4. 在本机构建环境安全配置上述正式签名、公证凭据，重新打包，再检查签名、Gatekeeper 评估和公证票据。公证凭据通过本机或 CI Secret 管理，不粘贴到聊天或源码中。当前已有的 ad-hoc 预览包不会因创建证书而自动转为已公证。

证书创建流程见 [Apple 官方说明](https://developer.apple.com/help/account/certificates/create-developer-id-certificates/)。

### 在本机钥匙串保存公证凭据

证书只能用于代码签名；提交 Apple 公证还需独立的账户认证。可以使用 `notarytool` 将认证信息保存在本机钥匙串，避免在构建命令、源码或聊天中传递密码：

```sh
# Apple ID 和 Team ID 替换为签名所属团队的信息；密码由工具隐藏提示输入
xcrun notarytool store-credentials more-wechat-notary \
  --apple-id "你的 Apple ID" --team-id "你的 Team ID"
```

密码使用 [Apple 账户后台生成的 App 专用密码](https://support.apple.com/zh-cn/102654)。本次本机另已准备仓库外的 `~/Library/Application Support/more-WeChat-signing/setup-notarization.command`，会交互读取 Apple ID，并让 Apple 工具隐藏读取密码。保存的 profile 名为 `more-wechat-notary`；随后使用 `--keychain-profile more-wechat-notary` 提交，只有 Apple 返回 Accepted 并完成票据装订、Gatekeeper 检查后，才能标记已公证。

## 文件与进程边界

- 只接受 `com.tencent.xinWeChat`、Apple 签名链以及 Tencent Team ID `5A4RE8SF68` 的原始 `.app`。自动查找系统与用户 Applications 目录中的 WeChat、微信、Weixin，手动选择其他目录也必须通过相同校验。其他官方签名身份目前应先验证再加入适配，不自动放宽条件。
- slot 1 使用原版；slot 2–5 位于 `~/Applications/more-WeChat/slot-N/WeChat.app`，稳定 Bundle ID 为 `com.tencent.xinWeChat.morewechat.slotN`。主进程按原版或已归属副本的准确可执行路径识别；含 `--type`、`--type=…` 或 `--mojo-platform-channel-handle=…` 的同可执行文件辅助角色也不计入数量。
- 根目录及每个 slot 都有归属 JSON。拒绝符号链接、无标记的既存目录，以及根目录以外的替换或递归清理。副本更新在根目录中的临时目录完成，签名验证后替换已停止的 slot；失败时回滚旧应用。旧副本辅助进程仍运行时也拒绝替换。
- 原版应用与 `~/Library/Containers`、`~/Library/Group Containers` 不会被删除或改写。稳定 Bundle ID 用于保留副本容器身份；代码不迁移、复制或检查聊天数据。实际微信是否接受独立容器需实机验证。
- 版本指纹覆盖 Info.plist、主可执行文件及签名 CodeResources。指纹、版本、原版路径或适配规则修订变化时，界面提示旧副本；仅在下一次启动已停止的对应 slot 时刷新。旧版没有规则修订字段的副本会自动视为待刷新，Bundle ID 和容器身份保持不变。运行中的副本保留不变。

## 本地签名策略与已知限制

使用 `ditto` 复制应用，只修改最外层 Info.plist，再从源签名提取权限。保留源 App Sandbox、网络、媒体等普通权限；移除 Tencent 专属的 application identifier、Team ID、App Groups、keychain access groups 及推送权限。遇到其他尚未适配的 developer/private 受限权限则停止。不会增加 `get-task-allow`、关闭 SIP、调用 sudo 或清除全局隔离属性。

移除腾讯 App Groups 后，微信内置 Chromium 与 crashpad 的继承沙盒会拒绝自己的 Mach 服务注册。4.1.13 的实机拒绝记录与崩溃栈均指向 `bootstrap_check_in`。v0.2.1 为每个 slot 的 `5A4RE8SF68.com.tencent.xinWeChat.morewechat.slotN.MachPortRendezvousServer.` 和 `.crashpad.child_port_handshake.` 两个准确前缀增加注册、查找规则，保留源 `temporary-exception.sbpl` 中的其他规则。新增规则不覆盖原版或其他 slot，不恢复腾讯 App Groups，也不关闭沙盒；嵌套组件通过原有 `com.apple.security.inherit` 继承规则。规则依据本机 `application.sb` 及 [Apple Mach 服务临时例外说明](https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/AppSandboxTemporaryExceptionEntitlements.html)。

对最外层应用进行 ad-hoc 签名，保留已有 runtime 标记，并启用 `disable-library-validation` 以允许未改写的 Tencent 嵌套库。签名时不使用 `--deep`；嵌套 app/framework 保持原版内容与签名。`--deep` 仅用于完整性验证。每个副本最终通过 `/usr/bin/open -n` 启动，应用服务随后等待持续存活的新主进程；命令退出成功不代表登录成功。

Apple 的 App Groups 和新系统的容器保护依赖签名团队身份，不能把腾讯专属权限直接用于 ad-hoc 签名。移除这些权限后，微信自身硬编码路径、共享组件、登录或通话仍可能不兼容；系统隐私权限也可能在更新后要求重新授予。当前实现报告失败/部分成功，不自动重置 TCC、处理钥匙串、修改数据目录或杀进程。

参考：[Apple 签名及嵌套代码说明](https://developer.apple.com/documentation/xcode/using-the-latest-code-signature-format)、[App Groups entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.application-groups)、[新版 macOS App Group 容器保护](https://developer.apple.com/documentation/xcode/accessing-app-group-containers)、[腾讯文档中的微信 Bundle ID](https://main.qcloudimg.com/raw/document/product/pdf/1092_105853_cn.pdf)。Team ID 应随每次实机兼容验收以 `codesign -dv` 对照发布包确认。

## 验收

模块内测试覆盖 slot 规划、稳定身份、权限过滤、指纹与规则修订失效、归属校验、同可执行文件辅助角色排除和 Unix 符号链接拒绝。2026-10-22 的 v0.2.1 修复在 Intel Mac 上通过 31 项 Rust 测试及 Clippy，并通过本机微信 4.1.13 从 1 个补齐为 2 个、再从 2 个补齐为 4 个主进程的正式编排检查；维护者确认四个独立窗口及登录、消息收发正常。四个主进程额外观察 60 秒保持稳定，再次启动目标 4 未新增实例。此前另已显式通过下述临时副本生成、签名测试；系统版本与人工验收边界见 [兼容记录](compatibility.md#macos-状态)。

macOS 测试另外使用临时 Mach-O 应用实际执行 `codesign`，验证内联身份表达式、拒绝错误身份，以及拒绝用 ad-hoc 签名冒充腾讯原版。以下显式测试从指定原版复制到临时目录、修改副本身份并本地签名；不启动微信、不访问聊天数据，测试结束后临时目录自动清理。需要约一份微信应用的额外磁盘空间。

```sh
MORE_WECHAT_TEST_SOURCE="/Applications/WeChat.app" \
  cargo test --locked --manifest-path src-tauri/Cargo.toml --lib \
  installed_wechat_can_prepare_signed_clone_without_launching -- --ignored --nocapture
```

扩大兼容范围时需要逐项验证：中文及空格路径；原版正在运行时打开独立窗口、多账号登录与消息收发；截图与通话；微信正常退出；原版升级后刷新停止的副本；运行副本及辅助进程拒绝刷新；权限拒绝/磁盘不足/签名失败保留旧应用；原版与原有聊天数据保持可用。兼容表应逐项记录已验与待验范围，不能从本机登录和消息正常推定其他场景通过。
