use crate::ip_probe::is_usable_public_ip;
use crate::net_bind;
use serde_json::Value;
use std::collections::HashSet;
use std::thread;
use std::time::{Duration, Instant};

const BASE: &str = "https://ilatency.com";

fn api_json(
    method: &str,
    path: &str,
    cookie: &str,
    body: Option<&Value>,
    timeout_secs: u64,
) -> Result<Value, String> {
    let url = format!("{BASE}{path}");
    let headers = [
        ("Cookie", cookie),
        ("Origin", BASE),
        ("Referer", "https://ilatency.com/dns-lookup"),
    ];
    net_bind::curl_json_direct(method, &url, &headers, body, timeout_secs)
}

/// Collect A-record candidates for a domain via iLatency multi-location DNS.
const CANCELLED: &str = crate::speed_select::CANCELLED;

/// Sleep in short slices; false if cancelled meanwhile.
fn sleep_unless_cancelled(total: Duration, cancelled: &dyn Fn() -> bool) -> bool {
    let deadline = Instant::now() + total;
    while Instant::now() < deadline {
        if cancelled() {
            return false;
        }
        thread::sleep(Duration::from_millis(100).min(deadline - Instant::now()));
    }
    !cancelled()
}

pub fn collect_a_records(
    host: &str,
    cookie: &str,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<String>, String> {
    let cookie = cookie.trim();
    if cookie.is_empty() {
        return Err("未配置 iLatency Cookie".into());
    }
    let host = host.trim();
    if host.is_empty() {
        return Err("域名为空".into());
    }

    if cancelled() {
        return Err(CANCELLED.into());
    }
    let session = api_json("GET", "/api/session", cookie, None, 20)?;
    if session
        .pointer("/user/username")
        .and_then(|v| v.as_str())
        .is_none()
    {
        return Err("iLatency 登录态无效，请重新粘贴 Cookie".into());
    }

    let payload = serde_json::json!({
        "task_type": "dns_lookup",
        "target": host,
        "province": "",
        "carrier": "",
        "count": 0,
        "timeout_ms": 8000,
        "options": {
            "region_scope": "country",
            "coverage_mode": "fast",
            "coverage_fallback_mode": "strict",
            "country_code": "CN",
            "include_overseas_anchors": true,
            "record_type": "A",
            "dns_resolver_mode": "cn_regional_auto",
            "show_dns_matching_details": false
        }
    });

    if cancelled() {
        return Err(CANCELLED.into());
    }
    let create_body = api_json("POST", "/api/ping-jobs", cookie, Some(&payload), 35)?;
    let job_id = create_body
        .pointer("/job/id")
        .or_else(|| create_body.get("id"))
        .and_then(|v| v.as_u64().or_else(|| v.as_i64().map(|n| n as u64)))
        .ok_or_else(|| format!("iLatency 未返回 job id: {create_body}"))?;

    let deadline = Instant::now() + Duration::from_secs(90);
    let mut last_status = String::from("queued");
    let mut final_body = create_body;

    while Instant::now() < deadline {
        if !sleep_unless_cancelled(Duration::from_millis(1200), cancelled) {
            return Err(CANCELLED.into());
        }
        let body = api_json(
            "GET",
            &format!("/api/ping-jobs/{job_id}"),
            cookie,
            None,
            25,
        )?;
        if cancelled() {
            return Err(CANCELLED.into());
        }
        final_body = body;
        last_status = final_body
            .pointer("/job/status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        if matches!(last_status.as_str(), "completed" | "failed" | "cancelled") {
            break;
        }
    }

    if last_status != "completed" {
        return Err(format!(
            "iLatency 任务未完成 (status={last_status}, job={job_id})"
        ));
    }

    let results = final_body
        .get("results")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut seen = HashSet::new();
    let mut ips = Vec::new();
    for item in results {
        if !item
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }
        let answers = item
            .pointer("/extra/answers")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for ans in answers {
            if ans.get("type").and_then(|v| v.as_str()) != Some("A") {
                continue;
            }
            if let Some(data) = ans.get("data").and_then(|v| v.as_str()) {
                let ip = data.trim();
                if is_usable_public_ip(ip) && seen.insert(ip.to_string()) {
                    ips.push(ip.to_string());
                }
            }
        }
    }

    if ips.is_empty() {
        Err(format!(
            "iLatency 完成但未收集到可用 A 记录 (job={job_id})"
        ))
    } else {
        Ok(ips)
    }
}

pub fn session_username(cookie: &str) -> Result<String, String> {
    let cookie = cookie.trim();
    if cookie.is_empty() {
        return Err("未配置 iLatency Cookie".into());
    }
    let body = api_json("GET", "/api/session", cookie, None, 20)?;
    body.pointer("/user/username")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "登录态无效".into())
}
