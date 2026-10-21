# 桌面版实现

React/TypeScript 只负责展示和调用固定 Tauri commands；Rust 负责校验、设置、平台适配、启动编排和诊断。应用无远程页面、网络请求、自动更新或任意 shell 执行接口。Tauri 单实例插件防止打开两个启动器同时修改副本。

## 界面与原生接口

| Command | 入参 | 结果 |
| --- | --- | --- |
| get_snapshot | 无 | 平台/系统、安装与版本、兼容状态、主进程、Mac 副本状态、设置和检测提示 |
| select_installation | path | 校验并保存选择，返回新 snapshot；无效路径不写入设置 |
| save_target | target | 保存 1–5 的目标总数 |
| launch | target | success / partial / failed / unchanged，包含前后数量、目标、结果说明及错误 |
| export_diagnostics | path | 手动选择的绝对 .json/.txt 路径，返回保存位置 |

错误返回 `{ code, message }`，常见 code 为 invalid_target、not_installed、unsupported_version、permission_denied、busy、launch_failed。`launch-progress` 事件包含 stage、message、current、target，长操作在阻塞工作线程运行，界面不被复制/签名任务阻塞。

数量以主进程观测为准。每次只新增一个，最多检测 20 轮（每轮间隔 750 ms，另加系统检测耗时），要求同一组新 PID 连续三次可见；结束后重新统计。已有数量达标直接返回。无法确认新主进程就停止继续新增；已有实例不会被工具关闭。未持续存活的新进程也不会被标成部分成功。进程稳定不代表两个账号隔离或已登录。

`Adapter` 抽象安装检测、兼容预检查、主进程枚举、Mac 副本状态和单次启动。Rust 编排测试使用可控适配器验证分支，真实进程行为单独记录。Mac 的副本签名和 Windows 的本机 API 不在浏览器演示中模拟成原生成功。

## 文件边界

设置通过临时文件和原子替换保存；损坏设置加载默认值并显示提示，读取时保留原文件。手动导出的诊断先对结构化字段脱敏再序列化，防止 Windows 反斜杠转义导致用户名遗漏或生成无效 JSON。

Mac 使用归属标记限定可替换的目录和 slot；遇到符号链接、未知目录或运行中的副本会停止。Windows 只在本用户会话、匹配主程序路径/启动时间的进程中查询候选锁；只允许明确配置的构建策略。

旧脚本仅寻找并打开桌面应用，复杂校验统一在 Rust 层完成，不再维护第二套多开实现。
