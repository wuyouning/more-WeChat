use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub install_path: Option<String>,
    pub target_count: u8,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            install_path: None,
            target_count: 2,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Installation {
    pub path: String,
    pub executable: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Compatibility {
    pub status: String,
    pub message: String,
    pub can_multi_launch: bool,
}
impl Compatibility {
    pub fn unverified(message: impl Into<String>, can_multi_launch: bool) -> Self {
        Self {
            status: "unverified".into(),
            message: message.into(),
            can_multi_launch,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Instance {
    pub pid: u32,
    pub executable: String,
    pub slot: Option<u8>,
    pub started_at: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CloneInfo {
    pub slot: u8,
    pub path: String,
    pub outdated: bool,
    pub running: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub platform: String,
    pub os_version: String,
    pub installation: Option<Installation>,
    pub compatibility: Compatibility,
    pub instances: Vec<Instance>,
    pub clones: Vec<CloneInfo>,
    pub settings: Settings,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LaunchProgress {
    pub stage: String,
    pub message: String,
    pub current: usize,
    pub target: u8,
}

#[derive(Clone, Debug, Serialize)]
pub struct LaunchResult {
    pub status: String,
    pub before: usize,
    pub after: usize,
    pub target: u8,
    pub message: String,
    pub errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}
impl AppError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
    pub fn io(context: &str, error: std::io::Error) -> Self {
        Self::new(
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                "permission_denied"
            } else {
                "io_error"
            },
            format!("{context}：{error}"),
        )
    }
}
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
impl std::error::Error for AppError {}
