//! macOS preview adapter. The original bundle and all user data remain untouched.
//! A successful `open` is deliberately not treated as a successful launch here;
//! the application service verifies new, surviving main processes.

use super::Adapter;
use crate::model::{AppError, CloneInfo, Compatibility, Installation, Instance};
use plist::{Dictionary, Value};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};
use sysinfo::System;

const ORIGINAL_ID: &str = "com.tencent.xinWeChat";
const TENCENT_TEAM: &str = "5A4RE8SF68";
const OWNER: &str = "more-WeChat/macos-clones/v1";
const ROOT_MARKER: &str = ".more-wechat-owner.json";
const CLONE_MARKER: &str = ".more-wechat-clone.json";
const APP_NAME: &str = "WeChat.app";

pub struct MacAdapter;

#[derive(Debug)]
struct Bundle {
    path: PathBuf,
    executable: PathBuf,
    executable_name: String,
    bundle_id: String,
    version: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct RootMarker {
    owner: String,
    root: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CloneMarker {
    owner: String,
    root: PathBuf,
    slot: u8,
    bundle_id: String,
    source_bundle_id: String,
    source_path: PathBuf,
    source_version: String,
    source_fingerprint: String,
    executable_name: String,
}

impl Adapter for MacAdapter {
    fn detect(&self, selected: Option<&str>) -> Result<Option<Installation>, AppError> {
        if let Some(path) = selected {
            let bundle = original(Path::new(path))?;
            return Ok(Some(bundle.installation()));
        }
        let home = home_dir()?;
        let candidates = [
            PathBuf::from("/Applications/WeChat.app"),
            home.join("Applications/WeChat.app"),
            PathBuf::from("/Applications/微信.app"),
            home.join("Applications/微信.app"),
            PathBuf::from("/Applications/Weixin.app"),
            home.join("Applications/Weixin.app"),
        ];
        let mut first_error = None;
        for path in candidates {
            if !path
                .try_exists()
                .map_err(|e| AppError::io("检查微信安装位置失败", e))?
            {
                continue;
            }
            match original(&path) {
                Ok(bundle) => return Ok(Some(bundle.installation())),
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        match first_error {
            Some(error) => Err(error),
            None => Ok(None),
        }
    }

    fn instances(&self, installation: &Installation) -> Result<Vec<Instance>, AppError> {
        // No signature subprocess for each poll: detection/launch validate the source.
        let source = read_bundle(Path::new(&installation.path))?;
        require_original_id(&source.bundle_id)?;
        let mut executables = BTreeMap::from([(source.executable.clone(), 1)]);
        if let Some(root) = managed_root(false)? {
            for slot in 2..=5 {
                let directory = slot_path(&root, slot);
                if exists_no_follow(&directory)? {
                    let marker = read_clone_marker(&root, &directory, slot)?;
                    let bundle = read_managed_bundle(&directory, &marker)?;
                    executables.insert(bundle.executable, slot);
                }
            }
        }
        let system = System::new_all();
        let mut found = Vec::new();
        for (pid, process) in system.processes() {
            if helper_arguments(process.cmd()) {
                continue;
            }
            if let Some(executable) = process.exe().and_then(|path| path.canonicalize().ok()) {
                if let Some(slot) = executables.get(&executable) {
                    found.push(Instance {
                        pid: pid.as_u32(),
                        executable: executable.to_string_lossy().into_owned(),
                        slot: Some(*slot),
                        started_at: process.start_time(),
                    });
                }
            }
        }
        found.sort_by_key(|instance| (instance.slot, instance.pid));
        Ok(found)
    }

    fn compatibility(&self, installation: &Installation) -> Compatibility {
        let check = || -> Result<(), AppError> {
            let source = original(Path::new(&installation.path))?;
            require_version(&source.version)?;
            let _ = clone_entitlements(&source.path)?;
            Ok(())
        };
        match check() {
            Ok(()) => Compatibility::unverified(
                "微信 4.x 身份及签名预检查通过。独立副本多开尚未完成 macOS 实机验收；首次启动需按系统提示授权，登录、截图与通话兼容性待验证。", true),
            Err(error) => Compatibility {
                status: if matches!(error.code.as_str(), "unsupported_version" | "unsupported_entitlements") { "unsupported" } else { "unverified" }.into(),
                message: error.message, can_multi_launch: false,
            },
        }
    }

    fn clones(
        &self,
        installation: &Installation,
        instances: &[Instance],
    ) -> Result<Vec<CloneInfo>, AppError> {
        let Some(root) = managed_root(false)? else {
            return Ok(vec![]);
        };
        let source = read_bundle(Path::new(&installation.path))?;
        require_original_id(&source.bundle_id)?;
        let fingerprint = fingerprint(&source)?;
        let mut clones = Vec::new();
        for slot in 2..=5 {
            let directory = slot_path(&root, slot);
            if exists_no_follow(&directory)? {
                let marker = read_clone_marker(&root, &directory, slot)?;
                let _ = read_managed_bundle(&directory, &marker)?;
                clones.push(CloneInfo {
                    slot,
                    path: directory.join(APP_NAME).to_string_lossy().into_owned(),
                    outdated: is_outdated(&marker, &source, &fingerprint),
                    running: instances.iter().any(|instance| instance.slot == Some(slot)),
                });
            }
        }
        Ok(clones)
    }

    fn launch_one(
        &self,
        installation: &Installation,
        _instances: &[Instance],
        progress: &dyn Fn(&str),
    ) -> Result<(), AppError> {
        progress("验证微信安装身份及签名");
        let source = original(Path::new(&installation.path))?;
        // Re-read live processes before choosing a slot, independently of the UI snapshot.
        let current = self.instances(&source.installation())?;
        let slot = next_slot(&current)?;
        if slot == 1 {
            progress("正在启动原版微信，等待主进程确认");
            return open_bundle(&source.path);
        }
        require_version(&source.version)?;
        let entitlements = clone_entitlements(&source.path)?;
        let root =
            managed_root(true)?.ok_or_else(|| AppError::new("io_error", "无法创建副本目录"))?;
        let target = slot_path(&root, slot);
        let source_fingerprint = fingerprint(&source)?;
        let stale = if exists_no_follow(&target)? {
            let marker = read_clone_marker(&root, &target, slot)?;
            let _ = read_managed_bundle(&target, &marker)?;
            is_outdated(&marker, &source, &source_fingerprint)
        } else {
            true
        };
        if stale {
            progress(&format!(
                "正在准备第 {slot} 个独立副本；更新仅替换已停止的应用文件"
            ));
            prepare_clone(
                &root,
                &source,
                &source_fingerprint,
                slot,
                entitlements,
                progress,
            )?;
        }
        let marker = read_clone_marker(&root, &target, slot)?;
        let bundle = read_managed_bundle(&target, &marker)?;
        verify_signature(&bundle.path, None)?;
        ensure_stopped(&target)?;
        progress(&format!("正在启动第 {slot} 个副本，等待主进程确认"));
        open_bundle(&bundle.path)
    }
}

impl Bundle {
    fn installation(&self) -> Installation {
        Installation {
            path: self.path.to_string_lossy().into_owned(),
            executable: self.executable.to_string_lossy().into_owned(),
            version: self.version.clone(),
        }
    }
}

fn home_dir() -> Result<PathBuf, AppError> {
    let path = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| AppError::new("io_error", "无法确定当前用户目录"))?;
    path.canonicalize()
        .map_err(|e| AppError::io("无法读取当前用户目录", e))
}

fn exists_no_follow(path: &Path) -> Result<bool, AppError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::io("检查文件失败", error)),
    }
}

fn reject_symlink(path: &Path) -> Result<(), AppError> {
    if fs::symlink_metadata(path)
        .map_err(|e| AppError::io("读取文件属性失败", e))?
        .file_type()
        .is_symlink()
    {
        return Err(AppError::new(
            "unsafe_path",
            format!("拒绝处理符号链接：{}", path.display()),
        ));
    }
    Ok(())
}

fn read_bundle(path: &Path) -> Result<Bundle, AppError> {
    if !path.is_absolute() || path.extension().and_then(|v| v.to_str()) != Some("app") {
        return Err(AppError::new(
            "invalid_installation",
            "请选择微信 .app 应用目录",
        ));
    }
    reject_symlink(path)?;
    let path = path
        .canonicalize()
        .map_err(|e| AppError::io("读取微信目录失败", e))?;
    for item in [
        path.join("Contents"),
        path.join("Contents/Info.plist"),
        path.join("Contents/MacOS"),
    ] {
        reject_symlink(&item)?;
    }
    let info = Value::from_file(path.join("Contents/Info.plist")).map_err(|e| {
        AppError::new(
            "invalid_installation",
            format!("无法读取微信 Info.plist：{e}"),
        )
    })?;
    let info = info
        .as_dictionary()
        .ok_or_else(|| AppError::new("invalid_installation", "微信 Info.plist 格式无效"))?;
    let string = |key| {
        info.get(key)
            .and_then(Value::as_string)
            .map(str::to_owned)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::new("invalid_installation", format!("微信缺少 {key}")))
    };
    let executable_name = string("CFBundleExecutable")?;
    validate_executable_name(&executable_name)?;
    let executable = path.join("Contents/MacOS").join(&executable_name);
    reject_symlink(&executable)?;
    if !executable.is_file() {
        return Err(AppError::new("invalid_installation", "微信主程序不存在"));
    }
    let executable = executable
        .canonicalize()
        .map_err(|e| AppError::io("读取微信主程序失败", e))?;
    if executable.parent() != Some(path.join("Contents/MacOS").as_path()) {
        return Err(AppError::new("unsafe_path", "微信主程序路径超出应用目录"));
    }
    Ok(Bundle {
        path,
        executable,
        executable_name,
        bundle_id: string("CFBundleIdentifier")?,
        version: string("CFBundleShortVersionString")?,
    })
}

fn validate_executable_name(name: &str) -> Result<(), AppError> {
    let mut components = Path::new(name).components();
    if name.contains('/')
        || name.contains('\\')
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(AppError::new("invalid_installation", "微信主程序名称无效"));
    }
    Ok(())
}

fn require_original_id(id: &str) -> Result<(), AppError> {
    if id != ORIGINAL_ID {
        return Err(AppError::new(
            "invalid_installation",
            "所选应用不是原版微信；请选择腾讯发布的微信 .app",
        ));
    }
    Ok(())
}

fn require_version(version: &str) -> Result<(), AppError> {
    if version.split('.').next() != Some("4") {
        return Err(AppError::new(
            "unsupported_version",
            format!("微信 {version} 尚无副本启动适配；当前仅对 4.x 提供待验证的预览支持"),
        ));
    }
    Ok(())
}

fn original(path: &Path) -> Result<Bundle, AppError> {
    let bundle = read_bundle(path)?;
    require_original_id(&bundle.bundle_id)?;
    let requirement = format!("anchor apple generic and identifier \"{ORIGINAL_ID}\" and certificate leaf[subject.OU] = \"{TENCENT_TEAM}\"");
    verify_signature(&bundle.path, Some(&requirement))?;
    Ok(bundle)
}

fn run(command: &mut Command, context: &str) -> Result<Output, AppError> {
    let output = command.output().map_err(|e| AppError::io(context, e))?;
    if output.status.success() {
        return Ok(output);
    }
    let message = String::from_utf8_lossy(&output.stderr);
    let permission = message.to_lowercase();
    let code = if permission.contains("permission denied")
        || permission.contains("operation not permitted")
    {
        "permission_denied"
    } else {
        "launch_failed"
    };
    let detail = message.chars().take(1600).collect::<String>();
    Err(AppError::new(code, format!("{context}：{}", detail.trim())))
}

fn verify_signature(path: &Path, requirement: Option<&str>) -> Result<(), AppError> {
    let mut command = Command::new("/usr/bin/codesign");
    command.args(["--verify", "--deep", "--strict"]);
    if let Some(requirement) = requirement {
        command.arg("-R").arg(requirement);
    }
    run(
        command.arg(path),
        "应用签名验证失败（请使用腾讯原版安装，或重新生成副本）",
    )?;
    Ok(())
}

fn open_bundle(path: &Path) -> Result<(), AppError> {
    run(
        Command::new("/usr/bin/open").arg("-n").arg(path),
        "系统未能启动微信",
    )?;
    Ok(())
}

fn next_slot(instances: &[Instance]) -> Result<u8, AppError> {
    (1..=5)
        .find(|slot| {
            !instances
                .iter()
                .any(|instance| instance.slot == Some(*slot))
        })
        .ok_or_else(|| AppError::new("instance_limit", "最多支持 5 个微信实例"))
}

fn helper_arguments(arguments: &[std::ffi::OsString]) -> bool {
    // Some Chromium/WeChat utility roles reuse the main executable itself, so
    // an exact executable path alone is insufficient to count independent apps.
    arguments.iter().skip(1).any(|argument| {
        let argument = argument.to_string_lossy();
        argument.starts_with("--type=")
            || argument == "--type"
            || argument.starts_with("--mojo-platform-channel-handle=")
    })
}

fn fingerprint(source: &Bundle) -> Result<String, AppError> {
    let mut digest = Sha256::new();
    for file in [
        source.path.join("Contents/Info.plist"),
        source.executable.clone(),
        source.path.join("Contents/_CodeSignature/CodeResources"),
    ] {
        if !file
            .try_exists()
            .map_err(|e| AppError::io("读取微信版本指纹失败", e))?
        {
            continue;
        }
        reject_symlink(&file)?;
        let mut file = fs::File::open(file).map_err(|e| AppError::io("读取微信版本指纹失败", e))?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file
                .read(&mut buffer)
                .map_err(|e| AppError::io("计算微信版本指纹失败", e))?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        digest.update([0]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn is_outdated(marker: &CloneMarker, source: &Bundle, fingerprint: &str) -> bool {
    marker.source_path != source.path
        || marker.source_version != source.version
        || marker.source_fingerprint != fingerprint
}

fn clone_id(slot: u8) -> String {
    format!("{ORIGINAL_ID}.morewechat.slot{slot}")
}
fn slot_path(root: &Path, slot: u8) -> PathBuf {
    root.join(format!("slot-{slot}"))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, AppError> {
    reject_symlink(path)?;
    let file = fs::File::open(path).map_err(|e| AppError::io("读取副本归属标记失败", e))?;
    serde_json::from_reader(file)
        .map_err(|e| AppError::new("unsafe_path", format!("副本归属标记无效：{e}")))
}

fn write_json_new<T: Serialize>(path: &Path, value: &T) -> Result<(), AppError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| AppError::io("创建副本归属标记失败", e))?;
    let content =
        serde_json::to_vec_pretty(value).map_err(|e| AppError::new("io_error", e.to_string()))?;
    file.write_all(&content)
        .and_then(|_| file.sync_all())
        .map_err(|e| AppError::io("保存副本归属标记失败", e))
}

fn validate_root(root: &Path) -> Result<(), AppError> {
    reject_symlink(root)?;
    if !root.is_dir()
        || root
            .canonicalize()
            .map_err(|e| AppError::io("校验副本目录失败", e))?
            != root
    {
        return Err(AppError::new(
            "unsafe_path",
            "副本根目录必须是受管理的真实目录",
        ));
    }
    let marker: RootMarker = read_json(&root.join(ROOT_MARKER))?;
    if marker.owner != OWNER || marker.root != root {
        return Err(AppError::new(
            "unsafe_path",
            "副本目录不属于此工具；已停止文件操作",
        ));
    }
    Ok(())
}

fn managed_root(create: bool) -> Result<Option<PathBuf>, AppError> {
    let applications = home_dir()?.join("Applications");
    if !exists_no_follow(&applications)? {
        if !create {
            return Ok(None);
        }
        fs::create_dir(&applications).map_err(|e| AppError::io("创建用户应用目录失败", e))?;
    }
    reject_symlink(&applications)?;
    let applications = applications
        .canonicalize()
        .map_err(|e| AppError::io("校验用户应用目录失败", e))?;
    let root = applications.join("more-WeChat");
    if !exists_no_follow(&root)? {
        if !create {
            return Ok(None);
        }
        // create_dir deliberately refuses an existing path instead of adopting it.
        fs::create_dir(&root).map_err(|e| AppError::io("创建独立副本目录失败", e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
                .map_err(|e| AppError::io("设置副本目录权限失败", e))?;
        }
        write_json_new(
            &root.join(ROOT_MARKER),
            &RootMarker {
                owner: OWNER.into(),
                root: root.clone(),
            },
        )?;
    }
    validate_root(&root)?;
    Ok(Some(root))
}

fn checked_child(root: &Path, child: &Path) -> Result<(), AppError> {
    validate_root(root)?;
    // Every rename/remove target must be an immediate child, never an ancestor or
    // a path constructed from a manifest. Manifests only describe identity.
    if !child.is_absolute() || child.parent() != Some(root) || child.file_name().is_none() {
        return Err(AppError::new("unsafe_path", "拒绝操作副本根目录以外的路径"));
    }
    if exists_no_follow(child)? {
        reject_symlink(child)?;
        if !child.is_dir()
            || child
                .canonicalize()
                .map_err(|e| AppError::io("校验副本路径失败", e))?
                != child
        {
            return Err(AppError::new("unsafe_path", "副本路径不是预期的真实目录"));
        }
    }
    Ok(())
}

fn read_clone_marker(root: &Path, directory: &Path, slot: u8) -> Result<CloneMarker, AppError> {
    checked_child(root, directory)?;
    let marker: CloneMarker = read_json(&directory.join(CLONE_MARKER))?;
    if !(2..=5).contains(&slot)
        || marker.owner != OWNER
        || marker.root != root
        || marker.slot != slot
        || marker.source_bundle_id != ORIGINAL_ID
        || marker.bundle_id != clone_id(slot)
    {
        return Err(AppError::new(
            "unsafe_path",
            "独立副本缺少有效归属标记；已停止文件操作",
        ));
    }
    validate_executable_name(&marker.executable_name)?;
    Ok(marker)
}

fn read_managed_bundle(directory: &Path, marker: &CloneMarker) -> Result<Bundle, AppError> {
    let bundle = read_bundle(&directory.join(APP_NAME))?;
    if bundle.bundle_id != marker.bundle_id || bundle.executable_name != marker.executable_name {
        return Err(AppError::new("unsafe_path", "副本身份与归属标记不匹配"));
    }
    Ok(bundle)
}

fn ensure_stopped(directory: &Path) -> Result<(), AppError> {
    let system = System::new_all();
    let running = system
        .processes()
        .values()
        .filter_map(|process| process.exe())
        .any(|path| {
            path.starts_with(directory)
                || path
                    .canonicalize()
                    .map(|path| path.starts_with(directory))
                    .unwrap_or(false)
        });
    if running {
        return Err(AppError::new(
            "clone_running",
            "该独立副本或其辅助进程仍在运行，请先从微信正常退出该副本",
        ));
    }
    Ok(())
}

fn remove_owned(root: &Path, directory: &Path, slot: u8) -> Result<(), AppError> {
    let _ = read_clone_marker(root, directory, slot)?;
    ensure_stopped(directory)?;
    // remove_dir_all does not follow symlinks inside the owned tree. Never remove
    // ~/Library/Containers, Group Containers, or the original installation.
    fs::remove_dir_all(directory).map_err(|e| AppError::io("清理受管理的副本文件失败", e))
}

fn clone_entitlements(path: &Path) -> Result<Value, AppError> {
    let output = run(
        Command::new("/usr/bin/codesign")
            .args(["--display", "--entitlements", "-", "--xml"])
            .arg(path),
        "读取微信签名权限失败",
    )?;
    let value = if output.stdout.is_empty() {
        Value::Dictionary(Dictionary::new())
    } else {
        Value::from_reader(std::io::Cursor::new(output.stdout))
            .map_err(|e| AppError::new("invalid_signature", format!("无法解析微信签名权限：{e}")))?
    };
    sanitize_entitlements(value)
}

fn sanitize_entitlements(value: Value) -> Result<Value, AppError> {
    let mut entitlements = value
        .into_dictionary()
        .ok_or_else(|| AppError::new("invalid_signature", "微信签名权限格式无效"))?;
    // These belong to Tencent's certificate/provisioning identity, not a local
    // ad-hoc signature. In particular, retaining their App Groups can make
    // launchd reject a sandboxed process on newer macOS versions.
    for key in [
        "application-identifier",
        "com.apple.application-identifier",
        "com.apple.developer.team-identifier",
        "com.apple.security.application-groups",
        "keychain-access-groups",
        "aps-environment",
        "com.apple.developer.aps-environment",
        "com.apple.security.get-task-allow",
        "get-task-allow",
    ] {
        entitlements.remove(key);
    }
    if entitlements
        .keys()
        .any(|key| key.starts_with("com.apple.developer.") || key.starts_with("com.apple.private."))
    {
        return Err(AppError::new(
            "unsupported_entitlements",
            "此微信版本包含尚未适配的受限制签名权限，已停止生成副本",
        ));
    }
    // Keep sandbox/network/media permissions from the source. The root process
    // is ad-hoc signed while its intact nested Tencent libraries remain signed
    // by Tencent, requiring this exception if hardened runtime is present.
    entitlements.insert(
        "com.apple.security.cs.disable-library-validation".into(),
        Value::Boolean(true),
    );
    Ok(Value::Dictionary(entitlements))
}

fn unique_suffix() -> String {
    format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

fn prepare_clone(
    root: &Path,
    source: &Bundle,
    source_fingerprint: &str,
    slot: u8,
    entitlements: Value,
    progress: &dyn Fn(&str),
) -> Result<(), AppError> {
    let target = slot_path(root, slot);
    checked_child(root, &target)?;
    ensure_stopped(&target)?;
    let suffix = unique_suffix();
    let staging = root.join(format!(".slot-{slot}-staging-{suffix}"));
    let backup = root.join(format!(".slot-{slot}-backup-{suffix}"));
    checked_child(root, &staging)?;
    checked_child(root, &backup)?;
    if exists_no_follow(&staging)? || exists_no_follow(&backup)? {
        return Err(AppError::new(
            "unsafe_path",
            "副本临时目录已存在，已停止操作",
        ));
    }
    fs::create_dir(&staging).map_err(|e| AppError::io("创建副本临时目录失败", e))?;
    let marker = CloneMarker {
        owner: OWNER.into(),
        root: root.to_path_buf(),
        slot,
        bundle_id: clone_id(slot),
        source_bundle_id: source.bundle_id.clone(),
        source_path: source.path.clone(),
        source_version: source.version.clone(),
        source_fingerprint: source_fingerprint.into(),
        executable_name: source.executable_name.clone(),
    };
    // From this point onward cleanup is allowed only after reading this marker.
    write_json_new(&staging.join(CLONE_MARKER), &marker)?;
    let prepare = || -> Result<(), AppError> {
        let _ = read_clone_marker(root, &staging, slot)?;
        let app = staging.join(APP_NAME);
        run(
            Command::new("/usr/bin/ditto").arg(&source.path).arg(&app),
            "复制微信应用失败",
        )?;
        let copied = read_bundle(&app)?;
        require_original_id(&copied.bundle_id)?;
        // Verify the copied, still-unmodified Tencent signature. A valid new
        // outer ad-hoc signature alone would otherwise seal a torn source copy.
        let requirement = format!("anchor apple generic and identifier \"{ORIGINAL_ID}\" and certificate leaf[subject.OU] = \"{TENCENT_TEAM}\"");
        verify_signature(&app, Some(&requirement))?;
        if copied.version != source.version || fingerprint(&copied)? != source_fingerprint {
            return Err(AppError::new(
                "source_changed",
                "微信在复制期间发生更新，请重新检测后重试",
            ));
        }
        let info_path = app.join("Contents/Info.plist");
        let mut info = Value::from_file(&info_path)
            .map_err(|e| AppError::new("invalid_installation", e.to_string()))?;
        let dictionary = info
            .as_dictionary_mut()
            .ok_or_else(|| AppError::new("invalid_installation", "副本 Info.plist 格式无效"))?;
        dictionary.insert(
            "CFBundleIdentifier".into(),
            Value::String(marker.bundle_id.clone()),
        );
        dictionary.insert(
            "CFBundleDisplayName".into(),
            Value::String(format!("微信 · {slot}")),
        );
        dictionary.insert(
            "LSMultipleInstancesProhibited".into(),
            Value::Boolean(false),
        );
        let _ = read_clone_marker(root, &staging, slot)?;
        reject_symlink(&info_path)?;
        info.to_file_xml(&info_path)
            .map_err(|e| AppError::new("io_error", format!("写入副本身份失败：{e}")))?;
        let entitlement_path = staging.join("clone-entitlements.plist");
        let _ = read_clone_marker(root, &staging, slot)?;
        let mut entitlement_file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&entitlement_path)
            .map_err(|e| AppError::io("创建副本权限文件失败", e))?;
        entitlements
            .to_writer_xml(&mut entitlement_file)
            .map_err(|e| AppError::new("io_error", format!("写入副本权限失败：{e}")))?;
        entitlement_file
            .sync_all()
            .map_err(|e| AppError::io("保存副本权限文件失败", e))?;
        let signature_directory = app.join("Contents/_CodeSignature");
        if exists_no_follow(&signature_directory)? {
            reject_symlink(&signature_directory)?;
        }
        progress(&format!(
            "正在为第 {slot} 个副本进行本地签名，保留原版和嵌套组件"
        ));
        // Do not use --deep when signing: it would change nested Tencent code and
        // overwrite its individual entitlements. --deep is only for verification.
        run(
            Command::new("/usr/bin/codesign")
                .args([
                    "--force",
                    "--sign",
                    "-",
                    "--timestamp=none",
                    "--generate-entitlement-der",
                    "--preserve-metadata=flags,runtime",
                ])
                .arg("--identifier")
                .arg(&marker.bundle_id)
                .arg("--entitlements")
                .arg(&entitlement_path)
                .arg(&app),
            "独立副本本地签名失败",
        )?;
        verify_signature(&app, None)?;
        let _ = read_managed_bundle(&staging, &marker)?;
        // Detect an update made while ditto/signing was in progress. A partial
        // bundle is never promoted over the last valid clone.
        if fingerprint(source)? != source_fingerprint {
            return Err(AppError::new(
                "source_changed",
                "微信在复制期间发生更新，请重新检测后重试",
            ));
        }
        Ok(())
    };
    if let Err(error) = prepare() {
        let _ = remove_owned(root, &staging, slot);
        return Err(error);
    }
    // Re-check ownership and all live processes immediately before replacement.
    checked_child(root, &target)?;
    let _ = read_clone_marker(root, &staging, slot)?;
    ensure_stopped(&target)?;
    let had_target = exists_no_follow(&target)?;
    if had_target {
        let _ = read_clone_marker(root, &target, slot)?;
        checked_child(root, &backup)?;
        fs::rename(&target, &backup).map_err(|e| AppError::io("暂存旧副本失败", e))?;
        // Cover an external launch racing the pre-rename check. proc_pidpath may
        // retain the old path after rename, so examine both raw path prefixes.
        if let Err(error) = ensure_stopped(&target).and_then(|_| ensure_stopped(&backup)) {
            checked_child(root, &target)?;
            fs::rename(&backup, &target)
                .map_err(|e| AppError::io("恢复正在启动的旧副本失败", e))?;
            let _ = remove_owned(root, &staging, slot);
            return Err(error);
        }
    }
    if let Err(error) = fs::rename(&staging, &target) {
        let rollback = if had_target {
            read_clone_marker(root, &backup, slot).and_then(|_| {
                checked_child(root, &target)?;
                fs::rename(&backup, &target).map_err(|e| AppError::io("恢复旧副本失败", e))
            })
        } else {
            Ok(())
        };
        let _ = remove_owned(root, &staging, slot);
        return Err(match rollback {
            Ok(()) => AppError::io("安装新副本失败，旧副本已保留", error),
            Err(rollback_error) => AppError::new(
                "rollback_failed",
                format!(
                    "安装新副本失败：{error}；{}；旧副本保留在 {}",
                    rollback_error.message,
                    backup.display()
                ),
            ),
        });
    }
    if had_target {
        if let Err(error) = ensure_stopped(&target).and_then(|_| remove_owned(root, &backup, slot))
        {
            progress(&format!(
                "新副本已就绪；旧应用文件保留在 {}：{}",
                backup.display(),
                error.message
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(slot: u8) -> Instance {
        Instance {
            pid: slot as u32,
            executable: "test".into(),
            slot: Some(slot),
            started_at: 0,
        }
    }

    #[test]
    fn only_fill_empty_slots_and_never_exceed_five() {
        assert_eq!(next_slot(&[]).unwrap(), 1);
        assert_eq!(next_slot(&[process(1), process(3)]).unwrap(), 2);
        assert!(next_slot(&(1..=5).map(process).collect::<Vec<_>>()).is_err());
    }

    #[test]
    fn same_executable_utility_roles_do_not_count_as_main_instances() {
        for role in [
            "renderer",
            "gpu-process",
            "utility",
            "wxocr",
            "wxpublic",
            "wxutility",
            "xplayer",
        ] {
            assert!(helper_arguments(&[
                "/Applications/微信 工作.app/Contents/MacOS/WeChat".into(),
                format!("--type={role}").into(),
            ]));
        }
        assert!(helper_arguments(&[
            "WeChat".into(),
            "--type".into(),
            "renderer".into()
        ]));
        assert!(helper_arguments(&[
            "WeChat".into(),
            "--mojo-platform-channel-handle=42".into()
        ]));
        assert!(!helper_arguments(&[
            "/Applications/WeChat.app/Contents/MacOS/WeChat".into()
        ]));
        assert!(!helper_arguments(&["WeChat".into(), "-psn_0_1234".into()]));
        assert!(!helper_arguments(&["WeChat".into(), "--typewriter".into()]));
    }

    #[test]
    fn stable_identity_is_distinct_from_original_and_other_slots() {
        assert_eq!(clone_id(2), "com.tencent.xinWeChat.morewechat.slot2");
        assert_ne!(clone_id(2), ORIGINAL_ID);
        assert_ne!(clone_id(2), clone_id(3));
        assert!(require_original_id(&clone_id(2)).is_err());
        assert!(require_original_id("com.tencent.WeWorkMac").is_err());
    }

    #[test]
    fn reject_executable_path_traversal_and_unknown_versions() {
        for name in [
            "../WeChat",
            "foo/WeChat",
            "..",
            "/WeChat",
            "foo\\WeChat",
            "",
        ] {
            assert!(validate_executable_name(name).is_err(), "{name}");
        }
        assert!(validate_executable_name("微信").is_ok());
        assert!(require_version("4.1.0").is_ok());
        assert!(require_version("40.1").is_err());
        assert!(require_version("3.9.0").is_err());
    }

    #[test]
    fn sanitize_vendor_identity_without_disabling_sandbox_or_granting_debug() {
        let mut entitlements = Dictionary::new();
        entitlements.insert(
            "com.apple.security.app-sandbox".into(),
            Value::Boolean(true),
        );
        entitlements.insert(
            "com.apple.security.network.client".into(),
            Value::Boolean(true),
        );
        entitlements.insert(
            "com.apple.security.application-groups".into(),
            Value::Array(vec![Value::String("TENCENT.shared".into())]),
        );
        entitlements.insert(
            "com.apple.developer.team-identifier".into(),
            Value::String(TENCENT_TEAM.into()),
        );
        entitlements.insert(
            "com.apple.application-identifier".into(),
            Value::String(ORIGINAL_ID.into()),
        );
        entitlements.insert("keychain-access-groups".into(), Value::Array(vec![]));
        entitlements.insert(
            "com.apple.security.get-task-allow".into(),
            Value::Boolean(true),
        );
        let value = sanitize_entitlements(Value::Dictionary(entitlements)).unwrap();
        let result = value.as_dictionary().unwrap();
        assert_eq!(
            result
                .get("com.apple.security.app-sandbox")
                .and_then(Value::as_boolean),
            Some(true)
        );
        assert_eq!(
            result
                .get("com.apple.security.network.client")
                .and_then(Value::as_boolean),
            Some(true)
        );
        for key in [
            "com.apple.security.application-groups",
            "com.apple.developer.team-identifier",
            "com.apple.application-identifier",
            "keychain-access-groups",
            "com.apple.security.get-task-allow",
        ] {
            assert!(!result.contains_key(key));
        }
    }

    #[test]
    fn unknown_restricted_entitlements_fail_closed() {
        let mut entitlements = Dictionary::new();
        entitlements.insert("com.apple.developer.unknown".into(), Value::Boolean(true));
        assert!(sanitize_entitlements(Value::Dictionary(entitlements)).is_err());
    }

    fn fixture_root() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().canonicalize().unwrap();
        write_json_new(
            &root.join(ROOT_MARKER),
            &RootMarker {
                owner: OWNER.into(),
                root: root.clone(),
            },
        )
        .unwrap();
        (directory, root)
    }

    fn fixture_marker(root: &Path) -> CloneMarker {
        CloneMarker {
            owner: OWNER.into(),
            root: root.into(),
            slot: 2,
            bundle_id: clone_id(2),
            source_bundle_id: ORIGINAL_ID.into(),
            source_path: PathBuf::from("/Applications/WeChat.app"),
            source_version: "4.1.0".into(),
            source_fingerprint: "fingerprint".into(),
            executable_name: "WeChat".into(),
        }
    }

    fn fixture_bundle() -> (tempfile::TempDir, Bundle) {
        let directory = tempfile::tempdir().unwrap();
        let app = directory.path().join("中文 path WeChat.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(app.join("Contents/_CodeSignature")).unwrap();
        let mut info = Dictionary::new();
        info.insert(
            "CFBundleIdentifier".into(),
            Value::String(ORIGINAL_ID.into()),
        );
        info.insert("CFBundleExecutable".into(), Value::String("WeChat".into()));
        info.insert(
            "CFBundleShortVersionString".into(),
            Value::String("4.1.0".into()),
        );
        Value::Dictionary(info)
            .to_file_xml(app.join("Contents/Info.plist"))
            .unwrap();
        fs::write(app.join("Contents/MacOS/WeChat"), b"main executable").unwrap();
        fs::write(
            app.join("Contents/_CodeSignature/CodeResources"),
            b"signed resource catalog",
        )
        .unwrap();
        let bundle = read_bundle(&app).unwrap();
        (directory, bundle)
    }

    #[test]
    fn read_exact_main_executable_with_unicode_and_spaces() {
        let (_directory, source) = fixture_bundle();
        let helper = source
            .path
            .join("Contents/Helpers/Helper.app/Contents/MacOS");
        fs::create_dir_all(&helper).unwrap();
        fs::write(helper.join("WeChat"), b"same named helper").unwrap();
        assert_eq!(source.executable, source.path.join("Contents/MacOS/WeChat"));
        assert_ne!(source.executable, helper.join("WeChat"));
        let installation = source.installation();
        assert_eq!(installation.version, "4.1.0");
        assert!(installation.path.ends_with("中文 path WeChat.app"));
    }

    #[test]
    fn fingerprint_detects_same_version_signed_resource_updates() {
        let (_directory, source) = fixture_bundle();
        let before = fingerprint(&source).unwrap();
        fs::write(
            source.path.join("Contents/_CodeSignature/CodeResources"),
            b"changed resource catalog",
        )
        .unwrap();
        let after = fingerprint(&source).unwrap();
        assert_ne!(before, after);
        assert_eq!(source.version, "4.1.0");
    }

    #[test]
    fn cleanup_removes_only_the_owned_slot_and_keeps_other_data() {
        let (_temporary, root) = fixture_root();
        let slot = slot_path(&root, 2);
        fs::create_dir(&slot).unwrap();
        write_json_new(&slot.join(CLONE_MARKER), &fixture_marker(&root)).unwrap();
        fs::write(slot.join("owned application"), b"replaceable").unwrap();
        let unrelated = root.join("unrelated-data");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("data"), b"keep").unwrap();
        remove_owned(&root, &slot, 2).unwrap();
        assert!(!slot.exists());
        assert_eq!(fs::read(unrelated.join("data")).unwrap(), b"keep");
        assert!(remove_owned(&root, &unrelated, 2).is_err());
    }

    #[test]
    fn never_adopt_unmarked_or_foreign_paths() {
        let (_temporary, root) = fixture_root();
        assert!(checked_child(&root, &root).is_err());
        assert!(checked_child(&root, &root.join("../outside")).is_err());
        let slot = slot_path(&root, 2);
        fs::create_dir(&slot).unwrap();
        assert!(read_clone_marker(&root, &slot, 2).is_err());
        let mut marker = fixture_marker(&root);
        marker.owner = "someone else".into();
        write_json_new(&slot.join(CLONE_MARKER), &marker).unwrap();
        assert!(remove_owned(&root, &slot, 2).is_err());
        assert!(slot.exists());
    }

    #[test]
    fn source_identity_version_and_fingerprint_all_invalidate_clone() {
        let (_temporary, root) = fixture_root();
        let mut marker = fixture_marker(&root);
        let source = Bundle {
            path: marker.source_path.clone(),
            executable: PathBuf::new(),
            executable_name: "WeChat".into(),
            bundle_id: ORIGINAL_ID.into(),
            version: marker.source_version.clone(),
        };
        assert!(!is_outdated(&marker, &source, "fingerprint"));
        assert!(is_outdated(&marker, &source, "updated-framework-resources"));
        marker.source_version = "4.0.0".into();
        assert!(is_outdated(&marker, &source, "fingerprint"));
        marker.source_version = source.version.clone();
        marker.source_path = PathBuf::from("/other/WeChat.app");
        assert!(is_outdated(&marker, &source, "fingerprint"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_slots_and_manifests_are_rejected() {
        use std::os::unix::fs::symlink;
        let (_temporary, root) = fixture_root();
        let outside = tempfile::tempdir().unwrap();
        let slot = slot_path(&root, 2);
        symlink(outside.path(), &slot).unwrap();
        assert!(checked_child(&root, &slot).is_err());
        fs::remove_file(&slot).unwrap();
        fs::create_dir(&slot).unwrap();
        let marker = outside.path().join("marker.json");
        write_json_new(&marker, &fixture_marker(&root)).unwrap();
        symlink(&marker, slot.join(CLONE_MARKER)).unwrap();
        assert!(read_clone_marker(&root, &slot, 2).is_err());
    }
}
