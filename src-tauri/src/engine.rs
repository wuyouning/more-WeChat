use crate::{
    model::{AppError, Installation, LaunchProgress, LaunchResult},
    platform::Adapter,
};
use std::{collections::HashSet, time::Duration};

pub fn validate_target(target: u8) -> Result<(), AppError> {
    if (1..=5).contains(&target) {
        Ok(())
    } else {
        Err(AppError::new(
            "invalid_target",
            "目标实例数必须是 1 到 5 的整数",
        ))
    }
}

/// Wait for a new main process to survive multiple observations. Command submission
/// alone is never considered a successful launch, nor is a login claimed.
pub fn launch_to_target(
    adapter: &dyn Adapter,
    installation: &Installation,
    target: u8,
    progress: &dyn Fn(LaunchProgress),
    sleep: &dyn Fn(Duration),
) -> Result<LaunchResult, AppError> {
    validate_target(target)?;
    let mut instances = adapter.instances(installation)?;
    let before = instances.len();
    if before >= target as usize {
        return Ok(LaunchResult {
            status: "unchanged".into(),
            before,
            after: before,
            target,
            message: format!("已检测到 {before} 个实例，无需新增"),
            errors: vec![],
        });
    }
    if target > 1 && !adapter.compatibility(installation).can_multi_launch {
        return Err(AppError::new(
            "unsupported_version",
            adapter.compatibility(installation).message,
        ));
    }
    let mut errors = vec![];
    let mut confirmed_new = HashSet::new();
    let required_samples = adapter.startup_confirmation_samples().clamp(3, 20);
    while instances.len() < target as usize {
        let old_pids: HashSet<u32> = instances.iter().map(|instance| instance.pid).collect();
        let previous_count = instances.len();
        let report = |message: &str| {
            progress(LaunchProgress {
                stage: "preparing".into(),
                message: message.into(),
                current: previous_count,
                target,
            })
        };
        report("正在准备启动微信…");
        if let Err(error) = adapter.launch_one(installation, &instances, &report) {
            errors.push(error.message);
            break;
        }
        progress(LaunchProgress {
            stage: "checking".into(),
            message: "正在确认新实例持续运行…".into(),
            current: previous_count,
            target,
        });
        let mut stable_pids = HashSet::new();
        let mut stable_samples = 0;
        // Preserve the original startup allowance when a platform needs a longer
        // observation window after its first new PID appears.
        for _ in 0..(17 + required_samples) {
            sleep(Duration::from_millis(750));
            match adapter.instances(installation) {
                Ok(observed) => instances = observed,
                Err(error) => {
                    errors.push(error.message);
                    break;
                }
            }
            let new_pids: HashSet<u32> = instances
                .iter()
                .map(|i| i.pid)
                .filter(|pid| !old_pids.contains(pid))
                .collect();
            if instances.len() > previous_count && !new_pids.is_empty() {
                if new_pids == stable_pids {
                    stable_samples += 1;
                } else {
                    stable_samples = 1;
                }
                stable_pids = new_pids;
                if stable_samples >= required_samples {
                    break;
                }
            } else {
                stable_pids.clear();
                stable_samples = 0;
            }
        }
        if !errors.is_empty() {
            break;
        }
        if stable_samples < required_samples {
            errors.push("未检测到持续运行的新实例，微信可能拒绝了多开或启动后已退出".into());
            break;
        }
        confirmed_new.extend(stable_pids);
        progress(LaunchProgress {
            stage: "running".into(),
            message: format!("已检测到 {} 个微信实例", instances.len()),
            current: instances.len(),
            target,
        });
    }
    // Recheck at the end; count observed state, not the number of launch calls.
    match adapter.instances(installation) {
        Ok(observed) => instances = observed,
        Err(error) => errors.push(error.message),
    }
    let after = instances.len();
    if after < target as usize && errors.is_empty() {
        errors.push("新实例在启动确认后退出，结束检查时未达到目标数量".into());
    }
    let reached = after >= target as usize && errors.is_empty();
    let status = if reached {
        "success"
    } else if instances
        .iter()
        .any(|instance| confirmed_new.contains(&instance.pid))
    {
        "partial"
    } else {
        "failed"
    };
    let message = if reached {
        format!("已检测到 {after} 个微信实例，请在微信窗口中登录")
    } else {
        format!(
            "目标 {target} 个，当前检测到 {after} 个实例；{}",
            if status == "partial" {
                "部分启动成功"
            } else {
                "未能完成启动"
            }
        )
    };
    Ok(LaunchResult {
        status: status.into(),
        before,
        after,
        target,
        message,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Compatibility, Instance};
    use std::sync::Mutex;
    struct Fake {
        initial_count: usize,
        count: Mutex<usize>,
        calls: Mutex<usize>,
        fail_at: usize,
        supported: bool,
        instant_exit: bool,
        rotating_pid: bool,
        observations: Mutex<u32>,
        exit_after_observation: Option<u32>,
        confirmation_samples: usize,
    }
    impl Fake {
        fn new(count: usize) -> Self {
            Self {
                initial_count: count,
                count: Mutex::new(count),
                calls: Mutex::new(0),
                fail_at: usize::MAX,
                supported: true,
                instant_exit: false,
                rotating_pid: false,
                observations: Mutex::new(0),
                exit_after_observation: None,
                confirmation_samples: 3,
            }
        }
    }
    impl Adapter for Fake {
        fn detect(&self, _: Option<&str>) -> Result<Option<Installation>, AppError> {
            Ok(None)
        }
        fn instances(&self, _: &Installation) -> Result<Vec<Instance>, AppError> {
            let mut observations = self.observations.lock().unwrap();
            *observations += 1;
            let count = if self
                .exit_after_observation
                .is_some_and(|last| *observations > last)
            {
                self.initial_count
            } else {
                *self.count.lock().unwrap()
            };
            Ok((0..count)
                .map(|i| Instance {
                    pid: if self.rotating_pid {
                        *observations
                    } else {
                        i as u32 + 1
                    },
                    executable: "Weixin.exe".into(),
                    slot: None,
                    started_at: 0,
                })
                .collect())
        }
        fn compatibility(&self, _: &Installation) -> Compatibility {
            Compatibility::unverified("未适配", self.supported)
        }
        fn startup_confirmation_samples(&self) -> usize {
            self.confirmation_samples
        }
        fn launch_one(
            &self,
            _: &Installation,
            _: &[Instance],
            _: &dyn Fn(&str),
        ) -> Result<(), AppError> {
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            if *calls == self.fail_at {
                return Err(AppError::new("permission_denied", "权限不足"));
            }
            if !self.instant_exit {
                *self.count.lock().unwrap() += 1;
            }
            Ok(())
        }
    }
    fn installation() -> Installation {
        Installation {
            path: "Weixin.exe".into(),
            executable: "Weixin.exe".into(),
            version: "test".into(),
        }
    }
    fn run(fake: &Fake, target: u8) -> Result<LaunchResult, AppError> {
        launch_to_target(fake, &installation(), target, &|_| {}, &|_| {})
    }
    #[test]
    fn rejects_invalid_counts() {
        for count in [0, 6, 255] {
            assert_eq!(
                run(&Fake::new(0), count).unwrap_err().code,
                "invalid_target"
            );
        }
    }
    #[test]
    fn only_fills_missing_instances() {
        let fake = Fake::new(1);
        let result = run(&fake, 3).unwrap();
        assert_eq!(*fake.calls.lock().unwrap(), 2);
        assert_eq!(result.after, 3);
        assert_eq!(result.status, "success");
    }
    #[test]
    fn never_stops_existing_instances() {
        let fake = Fake::new(4);
        assert_eq!(run(&fake, 2).unwrap().status, "unchanged");
        assert_eq!(*fake.calls.lock().unwrap(), 0);
    }
    #[test]
    fn reports_partial_launch_and_error() {
        let mut fake = Fake::new(0);
        fake.fail_at = 2;
        let result = run(&fake, 3).unwrap();
        assert_eq!(result.status, "partial");
        assert_eq!(result.after, 1);
        assert!(result.errors[0].contains("权限"));
    }
    #[test]
    fn command_submission_is_not_success() {
        let mut fake = Fake::new(0);
        fake.instant_exit = true;
        let result = run(&fake, 2).unwrap();
        assert_eq!(result.status, "failed");
        assert_eq!(result.after, 0);
        assert_eq!(*fake.calls.lock().unwrap(), 1);
    }
    #[test]
    fn unknown_versions_do_not_release_locks() {
        let mut fake = Fake::new(1);
        fake.supported = false;
        assert_eq!(run(&fake, 2).unwrap_err().code, "unsupported_version");
        assert_eq!(*fake.calls.lock().unwrap(), 0);
    }
    #[test]
    fn unknown_version_can_open_single_instance() {
        let mut fake = Fake::new(0);
        fake.supported = false;
        assert_eq!(run(&fake, 1).unwrap().status, "success");
    }

    #[test]
    fn continually_replaced_process_is_not_partial_success() {
        let mut fake = Fake::new(0);
        fake.rotating_pid = true;
        let result = run(&fake, 1).unwrap();
        assert_eq!(result.status, "failed");
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn late_startup_crash_does_not_pass_longer_platform_confirmation() {
        let mut fake = Fake::new(1);
        fake.confirmation_samples = 20;
        fake.exit_after_observation = Some(8);
        let result = run(&fake, 2).unwrap();
        assert_eq!(result.status, "failed");
        assert_eq!(result.before, 1);
        assert_eq!(result.after, 1);
        assert!(!result.errors.is_empty());
        assert_eq!(*fake.calls.lock().unwrap(), 1);
    }

    #[test]
    fn exit_during_final_recheck_includes_an_error() {
        let mut fake = Fake::new(1);
        fake.exit_after_observation = Some(4);
        let result = run(&fake, 2).unwrap();
        assert_eq!(result.status, "failed");
        assert_eq!(result.after, 1);
        assert!(result.errors[0].contains("退出"));
    }
}
