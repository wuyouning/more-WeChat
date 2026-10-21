use crate::{
    engine,
    model::{AppError, Compatibility, LaunchProgress, LaunchResult, Settings, Snapshot},
    platform::{self, Adapter},
    storage,
};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Service {
    adapter: Box<dyn Adapter>,
    root: PathBuf,
    settings: Mutex<Settings>,
    notes: Vec<String>,
    events: Mutex<VecDeque<String>>,
    busy: AtomicBool,
}

struct BusyGuard<'a>(&'a AtomicBool);
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl Service {
    pub fn new(root: PathBuf) -> Self {
        let (settings, notes) = match storage::load_settings(&root) {
            Ok(settings) => (settings, vec![]),
            Err(error) => (Settings::default(), vec![error.message]),
        };
        Self {
            adapter: platform::native(),
            root,
            settings: Mutex::new(settings),
            notes,
            events: Mutex::new(VecDeque::new()),
            busy: AtomicBool::new(false),
        }
    }

    fn begin(&self) -> Result<BusyGuard<'_>, AppError> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| AppError::new("busy", "已有操作进行中，请等待完成"))?;
        Ok(BusyGuard(&self.busy))
    }

    fn log(&self, message: &str) {
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        if events.len() >= 200 {
            events.pop_front();
        }
        events.push_back(format!(
            "[{}] {message}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        ));
    }

    pub fn snapshot(&self) -> Result<Snapshot, AppError> {
        let settings = self
            .settings
            .lock()
            .map_err(|_| AppError::new("state_error", "读取设置失败"))?
            .clone();
        let mut notes = self.notes.clone();
        let installation = match self.adapter.detect(settings.install_path.as_deref()) {
            Ok(installation) => installation,
            Err(error) => {
                notes.push(error.message);
                None
            }
        };
        let (compatibility, instances, clones) = if let Some(installation) = &installation {
            let compatibility = self.adapter.compatibility(installation);
            let instances = match self.adapter.instances(installation) {
                Ok(instances) => instances,
                Err(error) => {
                    notes.push(error.message);
                    vec![]
                }
            };
            let clones = match self.adapter.clones(installation, &instances) {
                Ok(clones) => clones,
                Err(error) => {
                    notes.push(error.message);
                    vec![]
                }
            };
            (compatibility, instances, clones)
        } else {
            (
                Compatibility {
                    status: "unsupported".into(),
                    message: "未找到可用的微信安装，请手动选择安装位置".into(),
                    can_multi_launch: false,
                },
                vec![],
                vec![],
            )
        };
        Ok(Snapshot {
            platform: std::env::consts::OS.into(),
            os_version: sysinfo::System::long_os_version()
                .unwrap_or_else(|| std::env::consts::OS.into()),
            installation,
            compatibility,
            instances,
            clones,
            settings,
            notes,
        })
    }

    pub fn select_installation(&self, path: String) -> Result<Snapshot, AppError> {
        let _guard = self.begin()?;
        let installation = self
            .adapter
            .detect(Some(&path))?
            .ok_or_else(|| AppError::new("not_installed", "所选位置中没有微信"))?;
        let mut settings = self
            .settings
            .lock()
            .map_err(|_| AppError::new("state_error", "读取设置失败"))?;
        let next = Settings {
            install_path: Some(installation.path),
            target_count: settings.target_count,
        };
        storage::save_settings(&self.root, &next)?;
        *settings = next;
        drop(settings);
        self.log("已更新微信安装位置");
        self.snapshot()
    }

    pub fn save_target(&self, target: u8) -> Result<(), AppError> {
        engine::validate_target(target)?;
        let _guard = self.begin()?;
        let mut settings = self
            .settings
            .lock()
            .map_err(|_| AppError::new("state_error", "读取设置失败"))?;
        let next = Settings {
            install_path: settings.install_path.clone(),
            target_count: target,
        };
        storage::save_settings(&self.root, &next)?;
        *settings = next;
        Ok(())
    }

    pub fn launch(
        &self,
        target: u8,
        notify: &dyn Fn(LaunchProgress),
    ) -> Result<LaunchResult, AppError> {
        engine::validate_target(target)?;
        let _guard = self.begin()?;
        let settings = self
            .settings
            .lock()
            .map_err(|_| AppError::new("state_error", "读取设置失败"))?
            .clone();
        let installation = self
            .adapter
            .detect(settings.install_path.as_deref())?
            .ok_or_else(|| AppError::new("not_installed", "未找到微信，请先选择安装位置"))?;
        self.log(&format!(
            "开始启动；目标 {target}；微信 {}",
            installation.version
        ));
        let result = engine::launch_to_target(
            self.adapter.as_ref(),
            &installation,
            target,
            &|progress| {
                self.log(&progress.message);
                notify(progress);
            },
            &std::thread::sleep,
        );
        match &result {
            Ok(result) => {
                self.log(&result.message);
                for error in &result.errors {
                    self.log(error);
                }
            }
            Err(error) => self.log(&error.message),
        }
        result
    }

    pub fn export(&self, path: &Path) -> Result<String, AppError> {
        if !path.is_absolute()
            || !path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("txt") || e.eq_ignore_ascii_case("json"))
        {
            return Err(AppError::new(
                "invalid_path",
                "请将诊断报告保存到绝对路径的 .json 或 .txt 文件",
            ));
        }
        let snapshot = self.snapshot()?;
        let fingerprint = snapshot
            .installation
            .as_ref()
            .and_then(|installation| std::fs::read(&installation.executable).ok())
            .map(|bytes| {
                use sha2::{Digest, Sha256};
                format!("{:x}", Sha256::digest(bytes))
            });
        let events = self
            .events
            .lock()
            .map_err(|_| AppError::new("state_error", "读取诊断记录失败"))?
            .clone();
        let mut report = serde_json::json!({
            "application": "more-WeChat", "version": env!("CARGO_PKG_VERSION"), "architecture": std::env::consts::ARCH,
            "timestampUtcSeconds": SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
            "acceptance": "预览版；进程运行不代表已登录；真实双账号验收状态请参见兼容表",
            "snapshot": snapshot, "executableSha256": fingerprint, "events": events,
        });
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .ok();
        storage::redact_value(&mut report, home.as_deref());
        let text = serde_json::to_string_pretty(&report)
            .map_err(|e| AppError::new("export_failed", e.to_string()))?;
        storage::atomic_write(path, text.as_bytes())?;
        self.log("诊断报告已导出");
        Ok(path.to_string_lossy().into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_operation_rejected_and_guard_resets() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::new(root.path().into());
        let guard = service.begin().unwrap();
        assert!(matches!(service.begin(), Err(AppError { code, .. }) if code == "busy"));
        drop(guard);
        assert!(service.begin().is_ok());
    }
    #[test]
    fn export_cannot_overwrite_executable() {
        let root = tempfile::tempdir().unwrap();
        let service = Service::new(root.path().into());
        assert_eq!(
            service
                .export(&root.path().join("Weixin.exe"))
                .unwrap_err()
                .code,
            "invalid_path"
        );
    }
}
