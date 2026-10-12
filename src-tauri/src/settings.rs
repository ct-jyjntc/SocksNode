use crate::speed_select::SelectStrategy;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    /// Full Cookie header for ilatency.com (HttpOnly cookies from browser DevTools).
    #[serde(default)]
    pub ilatency_cookie: String,
    /// Upstream TCP health-check interval in seconds.
    #[serde(default = "default_health_interval")]
    pub health_interval_secs: u64,
    /// After a failover restart, suppress another failover for this many seconds.
    #[serde(default = "default_failover_cooldown")]
    pub failover_cooldown_secs: u64,
    /// IP selection strategy after multi-source DNS.
    #[serde(default)]
    pub select_strategy: SelectStrategy,
    /// How many lowest-latency IPs to shortlist for speed testing.
    #[serde(default = "default_top_k")]
    pub speed_top_k: u32,
    /// Bytes to download via temporary SOCKS for throughput test.
    #[serde(default = "default_speed_bytes")]
    pub speed_test_bytes: u64,
    /// Timeout seconds for each candidate speed test.
    #[serde(default = "default_speed_timeout")]
    pub speed_test_timeout_secs: u64,
    /// Timeout seconds for each latency probe attempt through a node's SOCKS.
    #[serde(default = "default_latency_timeout")]
    pub latency_timeout_secs: u64,
    /// Register app as a login item / autostart entry.
    #[serde(default)]
    pub launch_at_login: bool,
    /// When launched by the login item, hide the window (tray only).
    /// Ignored unless `launch_at_login` is on.
    #[serde(default)]
    pub start_minimized: bool,
    /// When the app launches, start all enabled nodes automatically.
    #[serde(default)]
    pub auto_start_nodes: bool,
}

fn default_health_interval() -> u64 {
    5
}
fn default_failover_cooldown() -> u64 {
    60
}
fn default_top_k() -> u32 {
    3
}
fn default_speed_bytes() -> u64 {
    // v2rayN default SpeedTestUrl is cachefly 50mb.test.
    50 * 1024 * 1024
}
fn default_speed_timeout() -> u64 {
    // v2rayN default timeout is often 10s; we allow more headroom for slow exits.
    30
}
fn default_latency_timeout() -> u64 {
    8
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ilatency_cookie: String::new(),
            health_interval_secs: default_health_interval(),
            failover_cooldown_secs: default_failover_cooldown(),
            select_strategy: SelectStrategy::Throughput,
            speed_top_k: default_top_k(),
            speed_test_bytes: default_speed_bytes(),
            speed_test_timeout_secs: default_speed_timeout(),
            latency_timeout_secs: default_latency_timeout(),
            launch_at_login: false,
            start_minimized: false,
            auto_start_nodes: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsView {
    pub has_ilatency_cookie: bool,
    pub ilatency_cookie_preview: String,
    pub health_interval_secs: u64,
    pub failover_cooldown_secs: u64,
    pub ilatency_user: Option<String>,
    pub select_strategy: String,
    pub speed_top_k: u32,
    pub speed_test_bytes: u64,
    pub speed_test_timeout_secs: u64,
    pub latency_timeout_secs: u64,
    pub launch_at_login: bool,
    pub start_minimized: bool,
    pub auto_start_nodes: bool,
    /// Actual OS autostart registration status (may differ if user changed system settings).
    pub launch_at_login_active: bool,
}

/// Result of reading settings.json at startup.
pub struct SettingsLoad {
    pub settings: AppSettings,
    /// User-visible startup warning (corrupt / unreadable file).
    pub warning: Option<String>,
    /// The original file could not be read or moved aside: do not overwrite it.
    pub read_only: bool,
}

/// Describe a JSON error by position and kind only. serde's Display text can
/// quote field values (e.g. "invalid type: integer `123`"), which may be a
/// password or Cookie, so it must never reach logs or the UI.
pub fn json_error_summary(e: &serde_json::Error) -> String {
    use serde_json::error::Category;
    let kind = match e.classify() {
        Category::Data => "类型不匹配",
        Category::Syntax => "语法错误",
        Category::Eof => "文件不完整",
        Category::Io => "读取或校验失败",
    };
    if e.line() == 0 {
        kind.to_string()
    } else {
        format!("第 {} 行第 {} 列：{kind}", e.line(), e.column())
    }
}

/// Parse a config file: strip BOM, require UTF-8, summarise JSON errors.
pub fn parse_config<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    let raw = std::str::from_utf8(strip_utf8_bom(bytes))
        .map_err(|_| "文件不是有效的 UTF-8 文本".to_string())?;
    serde_json::from_str(raw).map_err(|e| json_error_summary(&e))
}

/// Drop a leading UTF-8 byte order mark (some editors on Windows add one).
pub fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

impl AppSettings {
    pub fn path(data_dir: &std::path::Path) -> PathBuf {
        data_dir.join("settings.json")
    }

    pub fn load(data_dir: &std::path::Path) -> SettingsLoad {
        let path = Self::path(data_dir);
        let fresh = |warning: Option<String>, read_only: bool| SettingsLoad {
            settings: Self::default(),
            warning,
            read_only,
        };
        if !path.exists() {
            return fresh(None, false);
        }
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(e) => {
                // Unreadable (e.g. EACCES) is not corruption: leave the file
                // alone and never write over it during this run.
                let msg = format!(
                    "设置文件无法读取（{e}），本次使用默认设置且不保存修改；请修复 {} 的读取权限后重启",
                    path.display()
                );
                eprintln!("[socks-node] {msg}");
                return fresh(Some(msg), true);
            }
        };
        let parsed = parse_config::<Self>(&bytes);
        match parsed {
            Ok(settings) => SettingsLoad {
                settings,
                warning: None,
                read_only: false,
            },
            Err(e) => {
                // Keep the broken file for inspection instead of silently
                // overwriting it with defaults on the next save.
                let ts = chrono::Utc::now().timestamp();
                let backup = data_dir.join(format!("settings.json.corrupt-{ts}"));
                let (msg, read_only) = match std::fs::rename(&path, &backup) {
                    Ok(()) => (
                        format!(
                            "设置文件无法解析（{e}），已备份为 {}，已恢复默认设置",
                            backup.display()
                        ),
                        false,
                    ),
                    Err(re) => (
                        format!(
                            "设置文件无法解析（{e}），备份失败（{re}）；本次使用默认设置且不保存修改；请修复或移走 {} 后重启",
                            path.display()
                        ),
                        true,
                    ),
                };
                eprintln!("[socks-node] {msg}");
                fresh(Some(msg), read_only)
            }
        }
    }

    /// Runtime health-check interval, same range as `update_settings`.
    pub fn health_interval(&self) -> u64 {
        self.health_interval_secs.clamp(3, 120)
    }

    /// Runtime failover cooldown, same range as `update_settings`.
    pub fn failover_cooldown(&self) -> u64 {
        self.failover_cooldown_secs.clamp(15, 600)
    }

    pub fn save(&self, data_dir: &std::path::Path) -> Result<(), String> {
        let path = Self::path(data_dir);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        crate::secure_store::write_private(&path, json.as_bytes())
    }

    pub fn to_view(&self, username: Option<String>, launch_at_login_active: bool) -> AppSettingsView {
        let cookie = self.ilatency_cookie.trim();
        let cookie_chars: Vec<char> = cookie.chars().collect();
        let preview = if cookie_chars.is_empty() {
            String::new()
        } else if cookie_chars.len() <= 18 {
            "***".into()
        } else {
            let head: String = cookie_chars[..8].iter().collect();
            let tail: String = cookie_chars[cookie_chars.len() - 6..].iter().collect();
            format!("{head}…{tail}")
        };
        AppSettingsView {
            has_ilatency_cookie: !cookie.is_empty(),
            ilatency_cookie_preview: preview,
            health_interval_secs: self.health_interval(),
            failover_cooldown_secs: self.failover_cooldown(),
            ilatency_user: username,
            select_strategy: self.select_strategy.as_str().to_string(),
            speed_top_k: self.speed_top_k.clamp(1, 8),
            speed_test_bytes: self.speed_test_bytes.clamp(1024 * 1024, 50 * 1024 * 1024),
            speed_test_timeout_secs: self.speed_test_timeout_secs.clamp(10, 60),
            latency_timeout_secs: self.latency_timeout_secs.clamp(2, 30),
            launch_at_login: self.launch_at_login,
            start_minimized: self.launch_at_login && self.start_minimized,
            auto_start_nodes: self.auto_start_nodes,
            launch_at_login_active,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsPatch {
    pub ilatency_cookie: Option<String>,
    pub health_interval_secs: Option<u64>,
    pub failover_cooldown_secs: Option<u64>,
    pub select_strategy: Option<String>,
    pub speed_top_k: Option<u32>,
    pub speed_test_bytes: Option<u64>,
    pub speed_test_timeout_secs: Option<u64>,
    pub latency_timeout_secs: Option<u64>,
    pub launch_at_login: Option<bool>,
    pub start_minimized: Option<bool>,
    pub auto_start_nodes: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_cookie_preview_is_utf8_safe() {
        let settings = AppSettings {
            ilatency_cookie: "session=测试测试测试测试测试测试测试测试测试测试".into(),
            ..AppSettings::default()
        };
        let view = settings.to_view(None, false);
        assert!(view.has_ilatency_cookie);
        assert!(view.ilatency_cookie_preview.contains('…'));
    }
}
