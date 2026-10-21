# macOS 独立副本预览适配

此版本未完成 macOS 实机验收。`canMultiLaunch` 表示 4.x、原版身份、Apple 签名及权限预检查通过，界面仍显示“版本未验证”；它不表示两个独立账号已经登录成功。发布兼容表必须保留“待验证”，直到 Apple Silicon、Intel 上分别记录实测微信版本和系统版本。

## 文件与进程边界

- 只接受 `com.tencent.xinWeChat`、Apple 签名链以及 Tencent Team ID `5A4RE8SF68` 的原始 `.app`。自动查找系统与用户 Applications 目录中的 WeChat、微信、Weixin，手动选择其他目录也必须通过相同校验。其他官方签名身份目前应先验证再加入适配，不自动放宽条件。
- slot 1 使用原版；slot 2–5 位于 `~/Applications/more-WeChat/slot-N/WeChat.app`，稳定 Bundle ID 为 `com.tencent.xinWeChat.morewechat.slotN`。主进程按原版或已归属副本的准确可执行路径识别；含 `--type`、`--type=…` 或 `--mojo-platform-channel-handle=…` 的同可执行文件辅助角色也不计入数量。
- 根目录及每个 slot 都有归属 JSON。拒绝符号链接、无标记的既存目录，以及根目录以外的替换或递归清理。副本更新在根目录中的临时目录完成，签名验证后替换已停止的 slot；失败时回滚旧应用。旧副本辅助进程仍运行时也拒绝替换。
- 原版应用与 `~/Library/Containers`、`~/Library/Group Containers` 不会被删除或改写。稳定 Bundle ID 用于保留副本容器身份；代码不迁移、复制或检查聊天数据。实际微信是否接受独立容器需实机验证。
- 版本指纹覆盖 Info.plist、主可执行文件及签名 CodeResources。指纹、版本或原版路径变化时，界面提示旧副本；仅在下一次启动已停止的对应 slot 时刷新。运行中的副本保留不变。

## 本地签名策略与已知限制

使用 `ditto` 复制应用，只修改最外层 Info.plist，再从源签名提取权限。保留源 App Sandbox、网络、媒体等普通权限；移除 Tencent 专属的 application identifier、Team ID、App Groups、keychain access groups 及推送权限。遇到其他尚未适配的 developer/private 受限权限则停止。不会增加 `get-task-allow`、关闭 SIP、调用 sudo 或清除全局隔离属性。

对最外层应用进行 ad-hoc 签名，保留已有 runtime 标记，并启用 `disable-library-validation` 以允许未改写的 Tencent 嵌套库。签名时不使用 `--deep`；嵌套 app/framework 保持原版内容与签名。`--deep` 仅用于完整性验证。每个副本最终通过 `/usr/bin/open -n` 启动，应用服务随后等待持续存活的新主进程；命令退出成功不代表登录成功。

Apple 的 App Groups 和新系统的容器保护依赖签名团队身份，不能把腾讯专属权限直接用于 ad-hoc 签名。移除这些权限后，微信自身硬编码路径、共享组件、登录或通话仍可能不兼容；系统隐私权限也可能在更新后要求重新授予。当前实现报告失败/部分成功，不自动重置 TCC、处理钥匙串、修改数据目录或杀进程。

参考：[Apple 签名及嵌套代码说明](https://developer.apple.com/documentation/xcode/using-the-latest-code-signature-format)、[App Groups entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.application-groups)、[新版 macOS App Group 容器保护](https://developer.apple.com/documentation/xcode/accessing-app-group-containers)、[腾讯文档中的微信 Bundle ID](https://main.qcloudimg.com/raw/document/product/pdf/1092_105853_cn.pdf)。Team ID 应随每次实机兼容验收以 `codesign -dv` 对照发布包确认。

## 验收

模块内纯测试覆盖 slot 规划、稳定身份、权限过滤、指纹失效、归属校验、同可执行文件辅助角色排除和 Unix 符号链接拒绝。Windows 上的独立测试 harness 已编译真实模块并通过 11 项测试及 Clippy；额外的 Unix 符号链接测试交由 macOS CI 执行。Windows 主机不能确认 Apple `codesign`、Launch Services、沙盒容器或微信登录的真实行为。

实机需要验证：中文及空格路径；原版正在运行时打开第二个独立窗口、两个测试账号登录；微信正常退出；原版升级后刷新停止的副本；运行副本及辅助进程拒绝刷新；权限拒绝/磁盘不足/签名失败保留旧应用；原版与原有聊天数据保持可用。兼容表只有通过这些场景后才可标记支持。
