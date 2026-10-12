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

impl AppSettings {
    pub fn path(data_dir: &std::path::Path) -> PathBuf {
        data_dir.join("settings.json")
    }

    pub fn load(data_dir: &std::path::Path) -> Self {
        let path = Self::path(data_dir);
        if !path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
            Err(_) => Self::default(),
        }
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
            health_interval_secs: self.health_interval_secs.max(3),
            failover_cooldown_secs: self.failover_cooldown_secs.max(15),
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
