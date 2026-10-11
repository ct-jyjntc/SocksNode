//! Force all SocksNode control-plane traffic onto a physical NIC.
//! Under Surge/Clash, system DNS often returns fake-ip (198.18.0.0/15); we never
//! trust getaddrinfo for upstream/API hosts. Resolve via DoH to literal resolvers
//! (e.g. 223.5.5.5) bound to the physical interface, then connect with
//! `curl --interface <if> --resolve host:443:real_ip`.

use std::ffi::CString;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;
use socket2::{Domain, Protocol, Socket, Type};

/// Remove short-lived curl header files, including on early return.
/// Their contents are mode 0600 and are never passed as process arguments.
struct TemporaryCurlHeaders(PathBuf);

impl Drop for TemporaryCurlHeaders {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[derive(Debug, Clone)]
pub struct PhysicalEgress {
    pub ifname: String,
    pub ifindex: u32,
    pub ipv4: Ipv4Addr,
}

fn is_virtual_ifname(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "lo"
        || n == "lo0"
        || n.starts_with("utun")
        || n.starts_with("awdl")
        || n.starts_with("llw")
        || n.starts_with("bridge")
        || n.starts_with("ap")
        || n.starts_with("gif")
        || n.starts_with("stf")
        || n.starts_with("anpi")
        || n.starts_with("anri")
        || n.starts_with("nan")
        || n.starts_with("ipsec")
        || n.starts_with("ppp")
        || n.starts_with("vmnet")
        || n.starts_with("veth")
        || n.starts_with("docker")
        || n.starts_with("tun")
        || n.starts_with("tap")
        || n.starts_with("wg")
}

fn if_index(name: &str) -> Option<u32> {
    let c = CString::new(name).ok()?;
    let idx = unsafe { libc::if_nametoindex(c.as_ptr()) };
    if idx == 0 {
        None
    } else {
        Some(idx)
    }
}

fn default_route_ifname() -> Option<String> {
    let out = Command::new("route")
        .args(["-n", "get", "default"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("interface:") {
            let name = rest.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

pub fn detect_physical_egress() -> Option<PhysicalEgress> {
    let mut physical = Vec::new();
    let Ok(ifaces) = if_addrs::get_if_addrs() else {
        return None;
    };
    for iface in ifaces {
        if iface.is_loopback() || is_virtual_ifname(&iface.name) {
            continue;
        }
        let IpAddr::V4(v4) = iface.ip() else {
            continue;
        };
        if v4.is_link_local() || v4.is_unspecified() {
            continue;
        }
        let Some(ifindex) = if_index(&iface.name) else {
            continue;
        };
        physical.push(PhysicalEgress {
            ifname: iface.name.clone(),
            ifindex,
            ipv4: v4,
        });
    }
    if physical.is_empty() {
        return None;
    }

    if let Some(def) = default_route_ifname() {
        if !is_virtual_ifname(&def) {
            if let Some(found) = physical.iter().find(|p| p.ifname == def) {
                return Some(found.clone());
            }
        }
    }

    physical.sort_by(|a, b| {
        let score = |n: &str| {
            if n == "en0" {
                0
            } else if n == "en1" {
                1
            } else if n.starts_with("en") {
                2
            } else if n.starts_with("eth") {
                3
            } else {
                4
            }
        };
        score(&a.ifname)
            .cmp(&score(&b.ifname))
            .then_with(|| a.ifname.cmp(&b.ifname))
    });
    physical.into_iter().next()
}

pub fn default_route_is_virtual() -> bool {
    default_route_ifname()
        .map(|n| is_virtual_ifname(&n))
        .unwrap_or(false)
}

pub fn egress_log_line() -> String {
    match detect_physical_egress() {
        Some(e) => {
            let def = default_route_ifname().unwrap_or_else(|| "?".into());
            let warn = if default_route_is_virtual() {
                " · WARNING default route is virtual"
            } else {
                ""
            };
            format!(
                "probe egress: {} ({}) ifindex={} default_route={}{warn}",
                e.ifname, e.ipv4, e.ifindex, def
            )
        }
        None => "probe egress: no physical IPv4 NIC found".into(),
    }
}

/// Cache the physical NIC briefly, not for the lifetime of the app. A Mac can
/// switch between Wi-Fi, Ethernet and hotspots while SocksNode stays in tray.
pub fn cached_egress() -> Option<PhysicalEgress> {
    type Cached = Option<(std::time::Instant, Option<PhysicalEgress>)>;
    static CACHE: OnceLock<parking_lot::Mutex<Cached>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| parking_lot::Mutex::new(None));
    {
        let cached = cache.lock();
        if let Some((at, value)) = cached.as_ref() {
            if at.elapsed() < Duration::from_secs(10) {
                return value.clone();
            }
        }
    }
    let current = detect_physical_egress();
    *cache.lock() = Some((std::time::Instant::now(), current.clone()));
    current
}

/// DoH resolvers as IP literals (never hostnames — avoids fake-ip for the resolver itself).
/// Prefer CN-reachable ones first; Cloudflare/Google often blocked on raw physical path in CN.
const DOH_IP_ENDPOINTS: &[&str] = &[
    "https://223.5.5.5/resolve?name={host}&type=A",
    "https://223.6.6.6/resolve?name={host}&type=A",
    "https://1.12.12.12/resolve?name={host}&type=A",
    "https://120.53.53.53/resolve?name={host}&type=A",
    "https://1.1.1.1/dns-query?name={host}&type=A",
    "https://8.8.8.8/resolve?name={host}&type=A",
];

fn curl_base(interface: Option<&str>) -> Command {
    let mut cmd = Command::new("curl");
    // Do not follow redirects while forwarding session Cookies. Redirecting
    // to a different host could disclose private headers or bypass --resolve.
    cmd.arg("-sS")
        .env("NO_PROXY", "*")
        .env("no_proxy", "*")
        .env("ALL_PROXY", "")
        .env("all_proxy", "")
        .env("HTTP_PROXY", "")
        .env("HTTPS_PROXY", "")
        .env("http_proxy", "")
        .env("https_proxy", "")
        .args(["--noproxy", "*"]);
    if let Some(iface) = interface {
        cmd.args(["--interface", iface]);
    }
    cmd
}

/// Resolve A records via DoH on the physical interface (no system DNS).
pub fn resolve_a_direct(host: &str) -> Result<Vec<String>, String> {
    let host = host.trim().trim_end_matches('.');
    if host.is_empty() {
        return Err("空主机名".into());
    }
    if host.parse::<IpAddr>().is_ok() {
        return Ok(vec![host.to_string()]);
    }
    let iface = cached_egress().map(|e| e.ifname.clone());
    let mut last_err = String::from("DoH 全部失败");
    let mut all = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for tmpl in DOH_IP_ENDPOINTS {
        let url = tmpl.replace("{host}", host);
        let mut cmd = curl_base(iface.as_deref());
        let output = cmd
            .args([
                "-m",
                "5",
                "-H",
                "Accept: application/dns-json",
                "-H",
                "User-Agent: SocksNode/1.0",
                &url,
            ])
            .output();
        let output = match output {
            Ok(o) => o,
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        if !output.status.success() {
            last_err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if last_err.is_empty() {
                last_err = format!("DoH HTTP fail for {url}");
            }
            continue;
        }
        let body: Value = match serde_json::from_slice(&output.stdout) {
            Ok(v) => v,
            Err(e) => {
                last_err = format!("DoH JSON: {e}");
                continue;
            }
        };
        if let Some(answers) = body.get("Answer").and_then(|v| v.as_array()) {
            for ans in answers {
                let ty = ans.get("type").and_then(|v| v.as_u64()).unwrap_or(0);
                if ty != 1 {
                    continue;
                }
                if let Some(data) = ans.get("data").and_then(|v| v.as_str()) {
                    let ip = data.trim().trim_end_matches('.');
                    // Reject fake-ip even from DoH (shouldn't happen with literal resolvers).
                    if crate::ip_probe::is_fake_ip(ip) {
                        continue;
                    }
                    if crate::ip_probe::is_usable_public_ip(ip) && seen.insert(ip.to_string()) {
                        all.push(ip.to_string());
                    }
                }
            }
        }
        if !all.is_empty() {
            // One successful DoH is enough for a single-view answer; callers that need
            // multi-location still use iLatency.
            break;
        }
    }

    if all.is_empty() {
        Err(last_err)
    } else {
        Ok(all)
    }
}

fn parse_http_host_port(url: &str) -> Result<(String, u16, bool), String> {
    let (https, rest) = if let Some(r) = url.strip_prefix("https://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("http://") {
        (false, r)
    } else {
        return Err(format!("不支持的 URL: {url}"));
    };
    let hostport = rest.split('/').next().unwrap_or(rest);
    let hostport = hostport.split('?').next().unwrap_or(hostport);
    if let Some((h, p)) = hostport.rsplit_once(':') {
        // Avoid splitting IPv6; we only use hostnames/IPv4 here.
        if !h.contains(']') && p.chars().all(|c| c.is_ascii_digit()) {
            let port: u16 = p.parse().map_err(|_| format!("端口无效: {p}"))?;
            return Ok((h.to_string(), port, https));
        }
    }
    Ok((hostport.to_string(), if https { 443 } else { 80 }, https))
}

/// HTTPS JSON via physical NIC: DoH-resolve host, then curl --interface --resolve.
pub fn curl_json_direct(
    method: &str,
    url: &str,
    extra_headers: &[(&str, &str)],
    body: Option<&Value>,
    timeout_secs: u64,
) -> Result<Value, String> {
    let (host, port, https) = parse_http_host_port(url)?;
    let scheme = if https { "https" } else { "http" };

    let iface = cached_egress()
        .map(|e| e.ifname.clone())
        .ok_or_else(|| "未找到物理网卡，无法直连".to_string())?;

    // Resolve real A (never system DNS).
    let ips = if host.parse::<IpAddr>().is_ok() {
        vec![host.clone()]
    } else {
        resolve_a_direct(&host)?
    };
    let ip = ips
        .first()
        .ok_or_else(|| format!("无法解析 {host}"))?
        .clone();

    let mut cmd = curl_base(Some(&iface));
    cmd.args([
        "-m",
        &timeout_secs.to_string(),
        "-X",
        method,
        "-H",
        "Accept: application/json",
        "-H",
        "User-Agent: SocksNode/1.0",
        "--resolve",
        &format!("{host}:{port}:{ip}"),
    ]);
    // Never put the iLatency session Cookie in curl's command line: ps can
    // reveal -H arguments. Curl supports reading headers from a private file.
    let mut header_lines = String::new();
    for (k, v) in extra_headers {
        if k.contains(['\r', '\n']) || v.contains(['\r', '\n']) {
            return Err("HTTP 头包含不允许的换行符".into());
        }
        header_lines.push_str(&format!("{k}: {v}\n"));
    }
    let _header_guard = if header_lines.is_empty() {
        None
    } else {
        let path = std::env::temp_dir().join(format!(
            "socksnode-curl-headers-{}.txt",
            uuid::Uuid::new_v4()
        ));
        crate::secure_store::write_private(&path, header_lines.as_bytes())?;
        cmd.args(["-H", &format!("@{}", path.display())]);
        Some(TemporaryCurlHeaders(path))
    };

    let output = if let Some(json) = body {
        cmd.args([
            "-H",
            "Content-Type: application/json",
            "-H",
            &format!("Idempotency-Key: {}", uuid::Uuid::new_v4()),
        ]);
        let payload = serde_json::to_vec(json).map_err(|e| e.to_string())?;
        let mut child = cmd
            .arg("--data-binary")
            .arg("@-")
            .arg(url)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("curl 启动失败: {e}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            if let Err(e) = stdin.write_all(&payload) {
                drop(stdin);
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("curl 请求体写入失败: {e}"));
            }
        } else {
            let _ = child.kill();
            let _ = child.wait();
            return Err("curl 标准输入不可用".into());
        }
        child.wait_with_output().map_err(|e| e.to_string())?
    } else {
        cmd.arg(url).output().map_err(|e| e.to_string())?
    };

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(if err.trim().is_empty() {
            format!(
                "curl 直连失败 ({scheme}://{host} via {iface}/{ip}, status={})",
                output.status
            )
        } else {
            format!("curl 直连失败 ({iface}/{ip}): {}", err.trim())
        });
    }

    let text = String::from_utf8_lossy(&output.stdout);
    if text.trim().is_empty() {
        return Err("curl 返回空响应".into());
    }
    // Response bodies may contain credentials; do not include them in logs.
    serde_json::from_str(text.trim()).map_err(|e| format!("JSON 解析失败: {e}"))
}

/// Result of a SOCKS download sample (body-only throughput).
#[derive(Debug, Clone)]
pub struct SocksDownloadSample {
    pub bytes: u64,
    /// Mbps using body-only time (time_total − time_starttransfer).
    pub mbps: f64,
}

/// curl download via **local SOCKS only**.
///
/// CRITICAL: do NOT pass `--noproxy '*'` here. On curl 8.x, `noproxy *` also
/// disables `--socks5-hostname` / `-x socks5h://`, so the request goes "direct"
/// and under Surge Enhanced Mode hits fake-ip (198.18.x) — measuring Surge's
/// path instead of the anytls exit. Clear proxy *env* vars instead.
///
/// Throughput uses **body-only** time (after first response byte).
pub fn curl_via_socks_download(
    socks_port: u16,
    url: &str,
    timeout: Duration,
    expected_bytes: Option<u64>,
) -> Result<SocksDownloadSample, String> {
    let socks = format!("127.0.0.1:{socks_port}");
    let max_secs = timeout.as_secs().max(8).to_string();
    // Intentionally NOT curl_base() — that injects `--noproxy *` which breaks SOCKS.
    let mut cmd = Command::new("curl");
    cmd.args(["-sS", "--http1.1"])
        .env("NO_PROXY", "")
        .env("no_proxy", "")
        .env("ALL_PROXY", "")
        .env("all_proxy", "")
        .env("HTTP_PROXY", "")
        .env("HTTPS_PROXY", "")
        .env("http_proxy", "")
        .env("https_proxy", "")
        .args([
            "-m",
            &max_secs,
            // socks5h = resolve hostname on the proxy side (remote exit DNS).
            "-x",
            &format!("socks5h://{socks}"),
            "--fail",
            "-o",
            "/dev/null",
            "-w",
            "%{size_download} %{time_total} %{time_starttransfer} %{time_pretransfer} %{http_code}",
            "-A",
            "SocksNode/1.0",
            url,
        ]);
    let output = cmd.output().map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let parts: Vec<&str> = stdout.split_whitespace().collect();
    let size: f64 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let time_total: f64 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let time_start: f64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let code: u32 = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);

    if !output.status.success() {
        let usable_partial = expected_bytes
            .map(|exp| size >= exp as f64 * 0.90)
            .unwrap_or(size >= 512.0 * 1024.0);
        if !usable_partial {
            let err = stderr.trim();
            return Err(if !err.is_empty() {
                format!("curl: {err} (got {size:.0}B total={time_total:.2}s http {code})")
            } else {
                format!("curl 失败 (got {size:.0}B total={time_total:.2}s http {code})")
            });
        }
    }

    if size < 256.0 * 1024.0 {
        return Err(format!(
            "下载过少 ({size:.0} bytes, http {code}) — 不完整测速作废"
        ));
    }
    if let Some(exp) = expected_bytes {
        if size < exp as f64 * 0.90 {
            return Err(format!(
                "下载不完整 ({size:.0}/{exp} bytes, {time_total:.2}s) — 作废"
            ));
        }
    }

    // Body-only: exclude DNS/TCP/TLS/anytls handshake before first byte.
    let mut body_secs = time_total - time_start;
    if body_secs < 0.05 {
        // Pathological: fall back to total but flag as weak.
        body_secs = time_total.max(0.05);
    }
    let mbps = size * 8.0 / body_secs / 1_000_000.0;
    if mbps > 2500.0 {
        return Err(format!("测速异常偏高 ({mbps:.1} Mbps)，已丢弃"));
    }
    Ok(SocksDownloadSample {
        bytes: size as u64,
        mbps,
    })
}

/// Back-compat helper.
#[allow(dead_code)]
pub fn curl_via_socks_mbps(
    socks_port: u16,
    url: &str,
    timeout: Duration,
) -> Result<f64, String> {
    curl_via_socks_download(socks_port, url, timeout, None).map(|s| s.mbps)
}

#[cfg(target_os = "macos")]
fn bind_socket_to_iface(socket: &Socket, egress: &PhysicalEgress, ipv4: bool) -> std::io::Result<()> {
    use std::os::unix::io::AsRawFd;
    let fd = socket.as_raw_fd();
    if ipv4 {
        let idx = egress.ifindex as libc::c_uint;
        let rc = unsafe {
            libc::setsockopt(
                fd,
                libc::IPPROTO_IP,
                25,
                &idx as *const _ as *const libc::c_void,
                std::mem::size_of_val(&idx) as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
    } else {
        let idx = egress.ifindex as libc::c_uint;
        let rc = unsafe {
            libc::setsockopt(
                fd,
                libc::IPPROTO_IPV6,
                125,
                &idx as *const _ as *const libc::c_void,
                std::mem::size_of_val(&idx) as libc::socklen_t,
            )
        };
        if rc != 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn bind_socket_to_iface(
    _socket: &Socket,
    _egress: &PhysicalEgress,
    _ipv4: bool,
) -> std::io::Result<()> {
    Ok(())
}

/// CLI flags for anytls-client to force physical egress.
pub fn anytls_bind_args() -> Vec<String> {
    match cached_egress() {
        Some(e) => vec![
            "-bind".into(),
            e.ipv4.to_string(),
            "-iface".into(),
            e.ifname.clone(),
        ],
        None => Vec::new(),
    }
}

fn tcping_once(ip_addr: IpAddr, port: u16, timeout: Duration) -> Option<u64> {
    let dest = SocketAddr::new(ip_addr, port);
    let domain = match ip_addr {
        IpAddr::V4(_) => Domain::IPV4,
        IpAddr::V6(_) => Domain::IPV6,
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP)).ok()?;
    let _ = socket.set_nodelay(true);

    let egress = cached_egress()?;
    if matches!(ip_addr, IpAddr::V4(_)) {
        let local = SocketAddr::new(IpAddr::V4(egress.ipv4), 0);
        socket.bind(&local.into()).ok()?;
        // LocalAddr bind is the primary anti-utun measure. IP_BOUND_IF is best-effort;
        // some paths are fine with LocalAddr alone.
        let _ = bind_socket_to_iface(&socket, &egress, true);
    }

    let start = std::time::Instant::now();
    match socket.connect_timeout(&dest.into(), timeout) {
        Ok(()) => {
            let ms = start.elapsed().as_millis() as u64;
            // <12ms on a public proxy port is almost always transparent-proxy noise.
            // >1500ms is near-timeout jitter — treat as fail so one spike doesn't win/lose ranking.
            if !(12..=1500).contains(&ms) {
                return None;
            }
            Some(ms)
        }
        Err(_) => None,
    }
}

/// TCP connect bound to physical egress.
/// Takes 3 samples and returns the **minimum** valid RTT (spike-resistant).
pub fn tcping_direct(ip: &str, port: u16, timeout: Duration) -> Option<u64> {
    let ip_addr: IpAddr = ip.parse().ok()?;
    if crate::ip_probe::is_fake_ip(ip) {
        return None;
    }
    let mut best: Option<u64> = None;
    for i in 0..3 {
        if let Some(ms) = tcping_once(ip_addr, port, timeout) {
            best = Some(match best {
                Some(b) => b.min(ms),
                None => ms,
            });
        }
        if i + 1 < 3 {
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    best
}
