use crate::net_bind::tcping_direct;
use std::collections::HashSet;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ProbeResult {
    pub ip: String,
    pub ms: Option<u64>,
}

/// TCP connect latency in ms. Binds to physical NIC when possible (bypass utun).
/// `None` means timeout/refused/error.
pub fn tcping(ip: &str, port: u16, timeout: Duration) -> Option<u64> {
    tcping_direct(ip, port, timeout)
}

/// Probe once; on failure retry up to `retries` additional times with `retry_gap`.
/// Returns first success latency, or None if all attempts fail.
pub fn tcping_with_retries(
    ip: &str,
    port: u16,
    timeout: Duration,
    retries: u32,
    retry_gap: Duration,
) -> Option<u64> {
    let attempts = retries.saturating_add(1);
    for i in 0..attempts {
        if let Some(ms) = tcping(ip, port, timeout) {
            return Some(ms);
        }
        if i + 1 < attempts {
            thread::sleep(retry_gap);
        }
    }
    None
}

/// Strip optional `[...]` around an IPv6 literal.
pub fn bare_host(host: &str) -> &str {
    let h = host.trim();
    match h.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
        Some(inner) if inner.parse::<std::net::Ipv6Addr>().is_ok() => inner,
        _ => h,
    }
}

pub fn is_ipv6_literal(host: &str) -> bool {
    bare_host(host).parse::<std::net::Ipv6Addr>().is_ok()
}

/// `host:port`, with IPv6 literals bracketed (`[2001:db8::1]:443`).
pub fn fmt_host_port(host: &str, port: u16) -> String {
    let h = bare_host(host);
    if is_ipv6_literal(h) {
        format!("[{h}]:{port}")
    } else {
        format!("{h}:{port}")
    }
}

/// The kernel always binds its outbound dialer to the physical NIC's IPv4
/// address (`-bind`, or auto-detected when omitted), so an IPv6 upstream can
/// not be dialed. Auto-selected candidates are therefore IPv4 only.
fn is_ipv4(ip: &str) -> bool {
    ip.parse::<std::net::Ipv4Addr>().is_ok()
}

pub fn is_domain_host(host: &str) -> bool {
    let host = host.trim().trim_matches(|c| c == '[' || c == ']');
    if host.is_empty() {
        return false;
    }
    host.parse::<IpAddr>().is_err()
}

pub fn is_usable_public_ip(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<IpAddr>() else {
        return false;
    };
    match addr {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            // 0.x / 127.x / 10.x / 224+
            if o[0] == 0 || o[0] == 10 || o[0] == 127 || o[0] >= 224 {
                return false;
            }
            // 172.16.0.0/12
            if o[0] == 172 && (16..=31).contains(&o[1]) {
                return false;
            }
            // 192.168.0.0/16
            if o[0] == 192 && o[1] == 168 {
                return false;
            }
            // 169.254.0.0/16 link-local
            if o[0] == 169 && o[1] == 254 {
                return false;
            }
            // 100.64.0.0/10 CGNAT
            if o[0] == 100 && (64..=127).contains(&o[1]) {
                return false;
            }
            // 198.18.0.0/15 benchmark / Surge-Clash fake-ip
            if o[0] == 198 && (18..=19).contains(&o[1]) {
                return false;
            }
            true
        }
        IpAddr::V6(v6) => {
            !v6.is_loopback()
                && !v6.is_unspecified()
                && !v6.is_multicast()
                && !v6.is_unique_local()
                && !v6.is_unicast_link_local()
        }
    }
}

/// System DNS via getaddrinfo (line-local view only for split DNS).
/// Filters Clash/Surge fake-ip ranges (198.18.0.0/15 etc.).
pub fn resolve_system(host: &str, port: u16) -> Result<Vec<String>, String> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("DNS 解析失败: {e}"))?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut fake = 0u32;
    let mut v6 = 0u32;
    for addr in addrs {
        let ip = addr.ip().to_string();
        if is_fake_ip(&ip) {
            fake += 1;
            continue;
        }
        if !is_ipv4(&ip) {
            v6 += 1;
            continue;
        }
        if is_ipv4(&ip) && is_usable_public_ip(&ip) && seen.insert(ip.clone()) {
            out.push(ip);
        }
    }
    if out.is_empty() {
        if fake > 0 {
            Err(format!(
                "系统 DNS 返回 fake-ip（{fake} 条，常见于 Surge/Clash），已忽略"
            ))
        } else if v6 > 0 {
            Err(format!(
                "域名只解析到 IPv6（{v6} 条）；内核出站绑定 IPv4 物理网卡，无法使用 IPv6，已忽略"
            ))
        } else {
            Err("域名未解析到可用 IP".into())
        }
    } else {
        Ok(out)
    }
}

/// Surge/Clash fake-ip / hijack ranges that must never be selected as upstream.
pub fn is_fake_ip(ip: &str) -> bool {
    let Ok(addr) = ip.parse::<IpAddr>() else {
        return false;
    };
    match addr {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            // 198.18.0.0/15 — benchmark / fake-ip pool
            if o[0] == 198 && (18..=19).contains(&o[1]) {
                return true;
            }
            // Some stacks also use 198.19.0.0/16 already covered above.
            false
        }
        IpAddr::V6(_) => false,
    }
}

/// DoH on physical NIC via IP-literal resolvers (never system DNS / fake-ip).
pub fn resolve_doh(host: &str, cancelled: &dyn Fn() -> bool) -> Vec<String> {
    crate::net_bind::resolve_a_direct_cancellable(host, cancelled).unwrap_or_default()
}

/// `resolve_system` in a helper thread so a stuck getaddrinfo can be
/// abandoned when the caller is cancelled (the thread finishes on its own).
pub fn resolve_system_cancellable(
    host: &str,
    port: u16,
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<String>, String> {
    let (tx, rx) = mpsc::channel();
    let host_owned = host.to_string();
    thread::spawn(move || {
        let _ = tx.send(resolve_system(&host_owned, port));
    });
    loop {
        if cancelled() {
            return Err(crate::speed_select::CANCELLED.into());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(result) => return result,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("DNS 解析线程异常退出".into())
            }
        }
    }
}

pub fn merge_unique(sets: impl IntoIterator<Item = Vec<String>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for set in sets {
        for ip in set {
            if is_ipv4(&ip) && is_usable_public_ip(&ip) && seen.insert(ip.clone()) {
                out.push(ip);
            }
        }
    }
    out
}

/// Parallel tcping; returns results sorted by latency ascending (failures last).
/// Stops launching new batches once `cancelled()` is true.
pub fn probe_all_cancellable(
    ips: &[String],
    port: u16,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Vec<ProbeResult> {
    if ips.is_empty() {
        return Vec::new();
    }
    let (tx, rx) = mpsc::channel();
    // Do not spawn one thread for every DNS result; a large iLatency response
    // should never exhaust the process's thread resources.
    for batch in ips.chunks(16) {
        if cancelled() {
            break;
        }
        thread::scope(|scope| {
            for ip in batch {
                let ip = ip.clone();
                let tx = tx.clone();
                scope.spawn(move || {
                    let ms = tcping(&ip, port, timeout);
                    let _ = tx.send(ProbeResult { ip, ms });
                });
            }
        });
    }
    drop(tx);
    let mut results: Vec<ProbeResult> = rx.into_iter().collect();
    results.sort_by(|a, b| match (a.ms, b.ms) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.ip.cmp(&b.ip),
    });
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_junk() {
        assert!(!is_usable_public_ip("198.18.0.97"));
        assert!(!is_usable_public_ip("192.168.1.1"));
        assert!(!is_usable_public_ip("10.0.0.1"));
        assert!(is_usable_public_ip("156.239.14.238"));
        assert!(is_domain_host("znjxdny.lajiaoyun.sbs"));
        assert!(!is_domain_host("1.2.3.4"));
    }
}
