use super::Adapter;
use crate::model::{AppError, Compatibility, Installation, Instance};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void,
    fs,
    io::Read,
    mem::size_of,
    os::windows::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Command,
    ptr::{null, null_mut},
};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, DuplicateHandle, DUPLICATE_CLOSE_SOURCE, DUPLICATE_SAME_ACCESS, HANDLE,
    },
    Security::{EqualSid, GetTokenInformation, TokenUser, TOKEN_QUERY, TOKEN_USER},
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            CreateMutexW, GetCurrentProcess, GetCurrentProcessId, OpenProcess, OpenProcessToken,
            PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};
use winreg::{
    enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
    RegKey,
};

pub struct WindowsAdapter;
const CREATE_NO_WINDOW: u32 = 0x08000000;

fn main_executable(path: &Path) -> bool {
    path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
        s.eq_ignore_ascii_case("WeChat.exe") || s.eq_ignore_ascii_case("Weixin.exe")
    })
}

fn helper_arguments(arguments: &[std::ffi::OsString]) -> bool {
    arguments.iter().skip(1).any(|argument| {
        let argument = argument.to_string_lossy().to_ascii_lowercase();
        argument.starts_with("--type=")
            || argument == "--type"
            || argument.starts_with("--mojo-platform-channel-handle=")
    })
}

fn normalized(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_lowercase()
}

fn inspect(path: &Path) -> Result<Installation, AppError> {
    let path = if path.is_dir() {
        [path.join("Weixin.exe"), path.join("WeChat.exe")]
            .into_iter()
            .find(|p| p.is_file())
            .ok_or_else(|| {
                AppError::new("not_installed", "所选文件夹中没有 Weixin.exe 或 WeChat.exe")
            })?
    } else {
        path.to_path_buf()
    };
    if !path.is_file() || !main_executable(&path) {
        return Err(AppError::new(
            "invalid_installation",
            "请选择官方微信的 Weixin.exe 或 WeChat.exe",
        ));
    }
    let canonical = path
        .canonicalize()
        .map_err(|e| AppError::io("无法读取微信安装路径", e))?;
    let version = file_version(&canonical)?;
    let path = canonical
        .to_string_lossy()
        .trim_start_matches(r"\\?\")
        .to_string();
    Ok(Installation {
        executable: path.clone(),
        path,
        version,
    })
}

fn system() -> System {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_exe(UpdateKind::Always)
            .with_cmd(UpdateKind::Always),
    );
    system
}

impl Adapter for WindowsAdapter {
    fn detect(&self, selected: Option<&str>) -> Result<Option<Installation>, AppError> {
        if let Some(path) = selected {
            return inspect(Path::new(path)).map(Some);
        }
        let mut candidates = vec![];
        for process in system().processes().values() {
            if let Some(path) = process.exe().filter(|path| main_executable(path)) {
                candidates.push(path.to_path_buf());
            }
        }
        for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
            for key in [
                r"Software\Tencent\Weixin",
                r"Software\Tencent\WeChat",
                r"Software\WOW6432Node\Tencent\Weixin",
                r"Software\WOW6432Node\Tencent\WeChat",
            ] {
                if let Ok(key) = RegKey::predef(hive).open_subkey(key) {
                    for name in ["InstallPath", "InstallLocation"] {
                        if let Ok(path) = key.get_value::<String, _>(name) {
                            candidates.push(PathBuf::from(path.trim_matches('"')));
                        }
                    }
                }
            }
        }
        for variable in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
            if let Some(root) = std::env::var_os(variable) {
                for relative in [r"Tencent\Weixin", r"Tencent\WeChat", "Weixin", "WeChat"] {
                    candidates.push(PathBuf::from(&root).join(relative));
                }
            }
        }
        for candidate in candidates {
            if let Ok(installation) = inspect(&candidate) {
                return Ok(Some(installation));
            }
        }
        Ok(None)
    }

    fn instances(&self, installation: &Installation) -> Result<Vec<Instance>, AppError> {
        let expected = normalized(Path::new(&installation.executable));
        let mut instances = vec![];
        let current_session = session(unsafe { GetCurrentProcessId() })?;
        for (pid, process) in system().processes() {
            let Some(path) = process.exe() else {
                if main_executable(Path::new(process.name()))
                    && session(pid.as_u32())? == current_session
                {
                    return Err(AppError::new(
                        "permission_denied",
                        "检测到无法读取的微信进程；若微信以管理员运行，请自行以相同权限重试",
                    ));
                }
                continue;
            };
            if !main_executable(path)
                || normalized(path) != expected
                || helper_arguments(process.cmd())
            {
                continue;
            }
            if session(pid.as_u32())? != current_session {
                continue;
            }
            let handle = open_process(pid.as_u32(), PROCESS_QUERY_LIMITED_INFORMATION)?;
            if !same_user(handle.0)? {
                continue;
            }
            instances.push(Instance {
                pid: pid.as_u32(),
                executable: path.to_string_lossy().into_owned(),
                slot: None,
                started_at: process.start_time(),
            });
        }
        instances.sort_by_key(|p| (p.started_at, p.pid));
        Ok(instances)
    }

    fn compatibility(&self, installation: &Installation) -> Compatibility {
        match rule_for(installation) {
            Ok(Some(rule)) => compatibility_for(
                &rule,
                System::os_version().as_deref(),
                std::env::consts::ARCH,
            ),
            Ok(None) => Compatibility::unverified(
                "此微信构建尚未配置实测启动策略，可启动单个实例并导出诊断；多开暂不可用",
                false,
            ),
            Err(error) => Compatibility::unverified(error.message, false),
        }
    }

    fn launch_one(
        &self,
        installation: &Installation,
        instances: &[Instance],
        progress: &dyn Fn(&str),
    ) -> Result<(), AppError> {
        if !instances.is_empty() {
            let rule = rule_for(installation)?.ok_or_else(|| {
                AppError::new(
                    "unsupported_version",
                    "该微信构建的启动策略尚未验证，已停止多开操作",
                )
            })?;
            progress("正在准备额外实例…");
            if rule.strategy == Strategy::Mutex {
                release_instance_locks(installation, instances, &rule)?;
            }
        }
        Command::new(&installation.executable)
            .current_dir(
                Path::new(&installation.executable)
                    .parent()
                    .ok_or_else(|| AppError::new("invalid_installation", "安装路径无效"))?,
            )
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| AppError::io("启动微信失败", e))?;
        Ok(())
    }
}

#[derive(Deserialize)]
struct Registry {
    rules: Vec<Rule>,
}
#[derive(Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Strategy {
    Normal,
    Mutex,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Rule {
    version: String,
    sha256: String,
    module_sha256: String,
    strategy: Strategy,
    mutex_names: Vec<String>,
    #[serde(default)]
    verified_environments: Vec<VerifiedEnvironment>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifiedEnvironment {
    os_version: String,
    architecture: String,
}

fn compatibility_for(rule: &Rule, os_version: Option<&str>, architecture: &str) -> Compatibility {
    // A matching binary only establishes an available launch strategy. Human
    // acceptance is scoped separately to the exact tested OS/architecture.
    let verified = rule.verified_environments.iter().any(|environment| {
        Some(environment.os_version.as_str()) == os_version
            && environment.architecture == architecture
    });
    if verified {
        return Compatibility {
            status: "verified".into(),
            message: "维护者已在此 Windows 系统与微信构建组合上确认：两个不同账号可同时登录并正常收发消息。截图、通话和其他系统版本仍待验证。".into(),
            can_multi_launch: true,
        };
    }
    Compatibility::unverified(
        if rule.strategy == Strategy::Normal {
            "此微信构建已有普通启动策略；当前系统与架构组合尚未完成双账号验收，将尝试启动并检测结果"
        } else {
            "此构建的启动锁已观测并配置；当前系统与架构组合尚未完成双账号验收，请先用测试账号验证"
        },
        true,
    )
}

fn rule_for(installation: &Installation) -> Result<Option<Rule>, AppError> {
    let registry: Registry = serde_json::from_str(include_str!("../../compatibility/windows.json"))
        .map_err(|e| AppError::new("invalid_compatibility", e.to_string()))?;
    let candidates: Vec<_> = registry
        .rules
        .into_iter()
        .filter(|r| r.version == installation.version)
        .collect();
    if candidates.is_empty() {
        return Ok(None);
    }
    let hash = hash_file(Path::new(&installation.executable))?;
    let filename = Path::new(&installation.executable)
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| AppError::new("invalid_installation", "微信程序路径无效"))?;
    let module = Path::new(&installation.executable)
        .parent()
        .unwrap()
        .join(&installation.version)
        .join(format!("{filename}.dll"));
    if !module.is_file() {
        return Ok(None);
    }
    let module_hash = hash_file(&module)?;
    Ok(candidates.into_iter().find(|rule| {
        rule.sha256 == hash
            && rule.module_sha256 == module_hash
            && (rule.strategy == Strategy::Normal || !rule.mutex_names.is_empty())
    }))
}

fn hash_file(path: &Path) -> Result<String, AppError> {
    let mut file = fs::File::open(path).map_err(|e| AppError::io("读取微信构建信息失败", e))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|e| AppError::io("计算微信构建指纹失败", e))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn expected_mutex(name: &str, mutex_names: &[String], session_id: u32) -> bool {
    let prefix = format!(r"\Sessions\{session_id}\BaseNamedObjects\");
    name.strip_prefix(&prefix).is_some_and(|suffix| {
        mutex_names
            .iter()
            .any(|expected| !expected.is_empty() && suffix == expected)
    })
}

struct OwnedHandle(HANDLE);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}
fn open_process(pid: u32, access: u32) -> Result<OwnedHandle, AppError> {
    let handle = unsafe { OpenProcess(access, 0, pid) };
    if handle.is_null() {
        Err(AppError::io(
            "读取微信进程失败；若微信以管理员运行，请自行以相同权限重试",
            std::io::Error::last_os_error(),
        ))
    } else {
        Ok(OwnedHandle(handle))
    }
}
fn session(pid: u32) -> Result<u32, AppError> {
    let mut value = 0;
    if unsafe { ProcessIdToSessionId(pid, &mut value) } == 0 {
        return Err(AppError::io(
            "读取会话失败",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(value)
}
fn user_token(process: HANDLE) -> Result<Vec<usize>, AppError> {
    let mut token = null_mut();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
        return Err(AppError::io(
            "读取进程身份失败",
            std::io::Error::last_os_error(),
        ));
    }
    let token = OwnedHandle(token);
    let mut needed = 0;
    unsafe {
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut needed);
    }
    let mut buffer = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    if needed == 0
        || unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        } == 0
    {
        return Err(AppError::io(
            "读取用户身份失败",
            std::io::Error::last_os_error(),
        ));
    }
    Ok(buffer)
}
fn same_user(process: HANDLE) -> Result<bool, AppError> {
    let current = user_token(unsafe { GetCurrentProcess() })?;
    let target = user_token(process)?;
    let current = unsafe { &*(current.as_ptr().cast::<TOKEN_USER>()) };
    let target = unsafe { &*(target.as_ptr().cast::<TOKEN_USER>()) };
    Ok(unsafe { EqualSid(current.User.Sid, target.User.Sid) } != 0)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HandleEntry {
    object: usize,
    pid: usize,
    value: usize,
    access: u32,
    trace: u16,
    type_index: u16,
    attributes: u32,
    reserved: u32,
}
#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *const u16,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQuerySystemInformation(
        class: u32,
        buffer: *mut c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
    fn NtQueryObject(
        handle: HANDLE,
        class: u32,
        buffer: *mut c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}

fn handle_snapshot() -> Result<Vec<HandleEntry>, AppError> {
    let mut length: usize = 1024 * 1024;
    loop {
        if length > 64 * 1024 * 1024 {
            return Err(AppError::new("resource_limit", "系统句柄数量超出诊断上限"));
        }
        let mut buffer = vec![0usize; length.div_ceil(size_of::<usize>())];
        let mut needed = 0;
        let status = unsafe {
            NtQuerySystemInformation(
                64,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * size_of::<usize>()) as u32,
                &mut needed,
            )
        };
        if status == 0xc0000004u32 as i32 {
            length = (needed as usize + 65536).max(length * 2);
            continue;
        }
        if status < 0 {
            return Err(AppError::new(
                "native_query_failed",
                format!("读取系统句柄失败（NTSTATUS {status:#x}）"),
            ));
        }
        let count = buffer[0];
        let available =
            (buffer.len() * size_of::<usize>() - 2 * size_of::<usize>()) / size_of::<HandleEntry>();
        if count > available {
            return Err(AppError::new("native_query_failed", "系统句柄结果长度无效"));
        }
        return Ok(unsafe {
            std::slice::from_raw_parts(buffer.as_ptr().add(2).cast::<HandleEntry>(), count)
        }
        .to_vec());
    }
}

fn object_name(handle: HANDLE) -> Option<String> {
    let mut buffer = vec![0usize; 1024];
    let mut needed = 0;
    let status = unsafe {
        NtQueryObject(
            handle,
            1,
            buffer.as_mut_ptr().cast(),
            (buffer.len() * size_of::<usize>()) as u32,
            &mut needed,
        )
    };
    if status < 0 {
        return None;
    }
    let value = unsafe { &*buffer.as_ptr().cast::<UnicodeString>() };
    let start = value.buffer as usize;
    let lower = buffer.as_ptr() as usize;
    let end = lower.checked_add(buffer.len() * size_of::<usize>())?;
    if value.length == 0
        || value.length % 2 != 0
        || !start.is_multiple_of(2)
        || start < lower
        || start.checked_add(value.length as usize)? > end
    {
        return None;
    }
    Some(String::from_utf16_lossy(unsafe {
        std::slice::from_raw_parts(value.buffer, value.length as usize / 2)
    }))
}

fn duplicate(source: HANDLE, value: usize, flags: u32) -> Result<OwnedHandle, AppError> {
    let mut target = null_mut();
    if unsafe {
        DuplicateHandle(
            source,
            value as HANDLE,
            GetCurrentProcess(),
            &mut target,
            0,
            0,
            flags,
        )
    } == 0
    {
        Err(AppError::io(
            "访问微信启动锁失败",
            std::io::Error::last_os_error(),
        ))
    } else {
        Ok(OwnedHandle(target))
    }
}

fn mutex_type(entries: &[HandleEntry], sentinel: HANDLE) -> Result<u16, AppError> {
    entries
        .iter()
        .find(|entry| {
            entry.pid == unsafe { GetCurrentProcessId() } as usize
                && entry.value == sentinel as usize
        })
        .map(|entry| entry.type_index)
        .ok_or_else(|| AppError::new("native_query_failed", "无法识别系统互斥锁类型"))
}

/// Read-only native probe: names of mutexes in matching executable processes in
/// this user's session. No handle is closed in the remote process by this API.
pub fn observe_mutexes(installation: &Installation) -> Result<Vec<(u32, String)>, AppError> {
    let instances = WindowsAdapter.instances(installation)?;
    let sentinel = OwnedHandle(unsafe { CreateMutexW(null(), 0, null()) });
    if sentinel.0.is_null() {
        return Err(AppError::io(
            "创建诊断互斥锁失败",
            std::io::Error::last_os_error(),
        ));
    }
    let entries = handle_snapshot()?;
    let kind = mutex_type(&entries, sentinel.0)?;
    let mut names = vec![];
    for instance in instances {
        let process = open_process(
            instance.pid,
            PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION,
        )?;
        for entry in entries
            .iter()
            .filter(|e| e.pid == instance.pid as usize && e.type_index == kind)
        {
            if let Ok(handle) = duplicate(process.0, entry.value, DUPLICATE_SAME_ACCESS) {
                if let Some(name) = object_name(handle.0) {
                    names.push((instance.pid, name));
                }
            }
        }
    }
    Ok(names)
}

fn release_instance_locks(
    installation: &Installation,
    instances: &[Instance],
    rule: &Rule,
) -> Result<(), AppError> {
    let sentinel = OwnedHandle(unsafe { CreateMutexW(null(), 0, null()) });
    if sentinel.0.is_null() {
        return Err(AppError::io(
            "读取启动锁失败",
            std::io::Error::last_os_error(),
        ));
    }
    let entries = handle_snapshot()?;
    let kind = mutex_type(&entries, sentinel.0)?;
    let current_session = session(unsafe { GetCurrentProcessId() })?;
    // Preflight every process before changing any remote handle.
    let expected_path = normalized(Path::new(&installation.executable));
    let processes = system();
    let mut candidates = vec![];
    for instance in instances {
        let current = processes
            .process(sysinfo::Pid::from_u32(instance.pid))
            .ok_or_else(|| AppError::new("process_changed", "微信进程已变化，请重新检测"))?;
        if current.start_time() != instance.started_at
            || current
                .exe()
                .is_none_or(|path| normalized(path) != expected_path)
            || session(instance.pid)? != current_session
        {
            return Err(AppError::new(
                "process_changed",
                "微信进程已变化，已停止操作",
            ));
        }
        let process = open_process(
            instance.pid,
            PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION,
        )?;
        if !same_user(process.0)? {
            return Err(AppError::new(
                "permission_denied",
                "不能操作其他用户的微信进程",
            ));
        }
        for entry in entries
            .iter()
            .filter(|e| e.pid == instance.pid as usize && e.type_index == kind)
        {
            let Ok(handle) = duplicate(process.0, entry.value, DUPLICATE_SAME_ACCESS) else {
                continue;
            };
            let Some(name) = object_name(handle.0) else {
                continue;
            };
            if expected_mutex(&name, &rule.mutex_names, current_session) {
                candidates.push((instance.pid, entry.value, entry.object, name));
            }
        }
    }
    if candidates.is_empty() {
        return Err(AppError::new(
            "mutex_not_found",
            "没有检测到此构建预期的启动锁，已停止多开；请导出诊断",
        ));
    }
    for (pid, value, object, name) in candidates {
        let process = open_process(pid, PROCESS_DUP_HANDLE | PROCESS_QUERY_LIMITED_INFORMATION)?;
        // A fresh table and name check narrow handle-reuse races. Windows has no
        // conditional-close primitive; version/hash gating remains essential.
        let fresh = handle_snapshot()?;
        if !fresh.iter().any(|e| {
            e.pid == pid as usize && e.value == value && e.object == object && e.type_index == kind
        }) {
            return Err(AppError::new("process_changed", "启动锁已变化，请重试"));
        }
        let duplicate_handle = duplicate(process.0, value, DUPLICATE_SAME_ACCESS)?;
        if object_name(duplicate_handle.0).as_deref() != Some(&name) {
            return Err(AppError::new("process_changed", "启动锁已变化，已停止操作"));
        }
        let _closed = duplicate(
            process.0,
            value,
            DUPLICATE_CLOSE_SOURCE | DUPLICATE_SAME_ACCESS,
        )?;
    }
    Ok(())
}

#[repr(C)]
struct FixedVersion {
    signature: u32,
    structure: u32,
    file_ms: u32,
    file_ls: u32,
    product_ms: u32,
    product_ls: u32,
    flags_mask: u32,
    flags: u32,
    os: u32,
    file_type: u32,
    subtype: u32,
    date_ms: u32,
    date_ls: u32,
}
#[link(name = "version")]
unsafe extern "system" {
    fn GetFileVersionInfoSizeW(path: *const u16, handle: *mut u32) -> u32;
    fn GetFileVersionInfoW(path: *const u16, handle: u32, length: u32, data: *mut c_void) -> i32;
    fn VerQueryValueW(
        data: *const c_void,
        block: *const u16,
        value: *mut *mut c_void,
        length: *mut u32,
    ) -> i32;
}
fn file_version(path: &Path) -> Result<String, AppError> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut unused = 0;
    let size = unsafe { GetFileVersionInfoSizeW(path.as_ptr(), &mut unused) };
    if size == 0 || size > 16 * 1024 * 1024 {
        return Err(AppError::new(
            "invalid_installation",
            "无法读取微信版本，请选择官方安装目录下的主程序",
        ));
    }
    let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    if unsafe { GetFileVersionInfoW(path.as_ptr(), 0, size, buffer.as_mut_ptr().cast()) } == 0 {
        return Err(AppError::io(
            "读取微信版本失败",
            std::io::Error::last_os_error(),
        ));
    }
    let mut value = null_mut();
    let mut length = 0;
    if unsafe {
        VerQueryValueW(
            buffer.as_ptr().cast(),
            [92u16, 0].as_ptr(),
            &mut value,
            &mut length,
        )
    } == 0
        || length < size_of::<FixedVersion>() as u32
        || value.is_null()
    {
        return Err(AppError::new("invalid_installation", "微信版本信息无效"));
    }
    let info = unsafe { &*value.cast::<FixedVersion>() };
    if info.signature != 0xfeef04bd {
        return Err(AppError::new("invalid_installation", "微信版本签名无效"));
    }
    Ok(format!(
        "{}.{}.{}.{}",
        info.file_ms >> 16,
        info.file_ms & 0xffff,
        info.file_ls >> 16,
        info.file_ls & 0xffff
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn helpers_are_not_main_processes() {
        assert!(main_executable(Path::new(r"C:\微信 工作\WEIXIN.EXE")));
        assert!(!main_executable(Path::new("WeixinExt.exe")));
        assert!(!main_executable(Path::new("WeChatAppEx.exe")));
    }
    #[test]
    fn same_executable_helpers_do_not_count() {
        for role in [
            "wxocr",
            "wxpublic",
            "wxutility",
            "xplayer",
            "renderer",
            "gpu-process",
        ] {
            let args = vec![
                "D:\\微信 工作\\Weixin.exe".into(),
                format!("--type={role}").into(),
            ];
            assert!(helper_arguments(&args));
        }
        assert!(!helper_arguments(&["D:\\微信 工作\\Weixin.exe".into()]));
        assert!(!helper_arguments(&[
            "Weixin.exe".into(),
            "--start-minimized".into()
        ]));
    }
    #[test]
    fn exact_mutex_in_own_session_only() {
        let names = vec!["_WeChat_App_Instance_Identity_Mutex_Name".to_string()];
        assert!(expected_mutex(
            r"\Sessions\1\BaseNamedObjects\_WeChat_App_Instance_Identity_Mutex_Name",
            &names,
            1
        ));
        assert!(!expected_mutex(
            r"\Sessions\2\BaseNamedObjects\_WeChat_App_Instance_Identity_Mutex_Name",
            &names,
            1
        ));
        assert!(!expected_mutex(
            r"\Sessions\1\BaseNamedObjects\prefix_WeChat_App_Instance_Identity_Mutex_Name",
            &names,
            1
        ));
        assert!(!expected_mutex(
            r"\BaseNamedObjects\_WeChat_App_Instance_Identity_Mutex_Name",
            &names,
            1
        ));
    }
    #[test]
    fn unknown_build_never_has_rule() {
        let installation = Installation {
            path: "missing".into(),
            executable: "missing".into(),
            version: "99.99.99".into(),
        };
        assert!(rule_for(&installation).unwrap().is_none());
    }

    fn acceptance_rule() -> Rule {
        serde_json::from_value(serde_json::json!({
            "version": "test", "sha256": "test", "moduleSha256": "test", "strategy": "normal", "mutexNames": [],
            "verifiedEnvironments": [{ "osVersion": "11 (22631)", "architecture": "x86_64" }]
        })).unwrap()
    }

    #[test]
    fn human_acceptance_matches_only_tested_environment() {
        let result = compatibility_for(&acceptance_rule(), Some("11 (22631)"), "x86_64");
        assert_eq!(result.status, "verified");
        assert!(result.can_multi_launch);
    }

    #[test]
    fn other_or_unknown_environments_remain_unverified() {
        for (os, arch) in [
            (Some("11 (26100)"), "x86_64"),
            (Some("10 (19045)"), "x86_64"),
            (Some("11 (22631)"), "aarch64"),
            (None, "x86_64"),
        ] {
            let result = compatibility_for(&acceptance_rule(), os, arch);
            assert_eq!(result.status, "unverified");
            assert!(result.can_multi_launch);
        }
    }

    #[test]
    fn strategy_without_acceptance_is_never_verified() {
        let mut rule = acceptance_rule();
        rule.verified_environments.clear();
        assert_eq!(
            compatibility_for(&rule, Some("11 (22631)"), "x86_64").status,
            "unverified"
        );
    }
}
