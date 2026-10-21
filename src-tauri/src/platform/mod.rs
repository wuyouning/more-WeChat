use crate::model::{AppError, CloneInfo, Compatibility, Installation, Instance};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::observe_mutexes as observe_windows_mutexes;

pub trait Adapter: Send + Sync {
    fn detect(&self, selected: Option<&str>) -> Result<Option<Installation>, AppError>;
    fn instances(&self, installation: &Installation) -> Result<Vec<Instance>, AppError>;
    fn compatibility(&self, installation: &Installation) -> Compatibility;
    fn clones(
        &self,
        _installation: &Installation,
        _instances: &[Instance],
    ) -> Result<Vec<CloneInfo>, AppError> {
        Ok(vec![])
    }
    fn launch_one(
        &self,
        installation: &Installation,
        instances: &[Instance],
        progress: &dyn Fn(&str),
    ) -> Result<(), AppError>;
}

pub fn native() -> Box<dyn Adapter> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsAdapter)
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacAdapter)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        compile_error!("more-WeChat supports Windows and macOS only");
    }
}
