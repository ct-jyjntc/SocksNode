use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use crate::ip_probe::ProbeResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum SelectStrategy {
    Balanced,
    Latency,
    /// Prefer higher throughput among reachable candidates (default).
    #[default]
    Throughput,
}

impl SelectStrategy {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "balanced" | "均衡" => Self::Balanced,
            "latency" | "lowlatency" | "低延迟" => Self::Latency,
            "throughput" | "bandwidth" | "高带宽" | "speed" => Self::Throughput,
            _ => Self::Throughput,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Balanced => "balanced",
            Self::Latency => "latency",
            Self::Throughput => "throughput",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Balanced => "均衡",
            Self::Latency => "低延迟",
            Self::Throughput => "高带宽",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScoredIp {
    pub ip: String,
    pub rtt_ms: u64,
    pub mbps: Option<f64>,
    pub score: f64,
    pub note: String,
}

/// Score higher = better.
pub fn score_candidate(strategy: SelectStrategy, rtt_ms: u64, mbps: Option<f64>) -> f64 {
    let rtt = rtt_ms.max(1) as f64;
    let speed = mbps.unwrap_or(0.0).max(0.0);
    match strategy {
        SelectStrategy::Latency => {
            // Lower RTT wins; tiny bonus for speed if present.
            1_000_000.0 / rtt + speed * 0.01
        }
        SelectStrategy::Throughput => {
            // Bandwidth first; break ties with latency.
            speed * 1000.0 + 1_000.0 / rtt
        }
        SelectStrategy::Balanced => {
            // Mbps / (1 + RTT/100)
            speed / (1.0 + rtt / 100.0)
        }
    }
}

pub fn pick_free_local_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    drop(listener);
    Ok(port)
}

/// Wait until TCP connect to 127.0.0.1:port succeeds or timeout.
fn wait_port_open(port: u16, timeout: Duration, cancelled: &dyn Fn() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cancelled() {
            return false;
        }
        if TcpStream::connect_timeout(
            &format!("127.0.0.1:{port}").parse().unwrap(),
            Duration::from_millis(200),
        )
        .is_ok()
        {
            return true;
        }
        thread::sleep(Duration::from_millis(80));
    }
    false
}

/// Registry of temporary speed-test kernels.
///
/// Every ephemeral anytls-client is owned by this table from spawn until it is
/// reaped, so Stop can kill a node's in-flight probe immediately and app
/// shutdown never leaves an orphan behind. Entries are keyed by a private,
/// never-reused token (not the pid), so a recycled pid can never make us
/// kill an unrelated process.
#[derive(Clone, Default)]
pub struct TempKernels {
    inner: Arc<Mutex<TempKernelTable>>,
}

#[derive(Default)]
struct TempKernelTable {
    next_token: u64,
    /// Set by `kill_all` (shutdown); later registrations are refused.
    closed: bool,
    entries: HashMap<u64, (String, Child)>,
}

impl TempKernels {
    /// Take ownership of a freshly spawned kernel. After shutdown started the
    /// child is killed right away and an error is returned.
    fn register(&self, owner: &str, child: Child) -> Result<u64, String> {
        let mut table = self.inner.lock();
        if table.closed {
            drop(table);
            let mut child = child;
            kill_child(&mut child);
            return Err("应用正在退出，已终止临时 anytls".into());
        }
        table.next_token += 1;
        let token = table.next_token;
        table.entries.insert(token, (owner.to_string(), child));
        Ok(token)
    }

    /// Kill (if still running), wait and forget one temporary kernel.
    fn reap(&self, token: u64) {
        let entry = self.inner.lock().entries.remove(&token);
        if let Some((_, mut child)) = entry {
            kill_child(&mut child);
        }
    }

    /// True while the kernel is registered and has not exited.
    fn is_running(&self, token: u64) -> bool {
        let mut table = self.inner.lock();
        match table.entries.get_mut(&token) {
            Some((_, child)) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Pids of live temporary kernels (never kill these as port squatters).
    pub fn pids(&self) -> Vec<u32> {
        self.inner
            .lock()
            .entries
            .values()
            .map(|(_, c)| c.id())
            .collect()
    }

    /// Kill every temporary kernel launched for `owner` (a node id).
    pub fn kill_owner(&self, owner: &str) {
        let victims: Vec<Child> = {
            let mut table = self.inner.lock();
            let tokens: Vec<u64> = table
                .entries
                .iter()
                .filter(|(_, (o, _))| o == owner)
                .map(|(t, _)| *t)
                .collect();
            tokens
                .into_iter()
                .filter_map(|t| table.entries.remove(&t).map(|(_, c)| c))
                .collect()
        };
        for mut child in victims {
            kill_child(&mut child);
        }
    }

    /// Kill and wait for all temporary kernels, and refuse new ones (shutdown).
    pub fn kill_all(&self) {
        let victims: Vec<Child> = {
            let mut table = self.inner.lock();
            table.closed = true;
            std::mem::take(&mut table.entries)
                .into_values()
                .map(|(_, c)| c)
                .collect()
        };
        for mut child in victims {
            kill_child(&mut child);
        }
    }
}

fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub const CANCELLED: &str = "已取消（节点已停止或状态已变化）";

/// Connection parameters for one temporary speed-test kernel.
pub struct ProbeKernel<'a> {
    pub kernel: &'a Path,
    pub password: &'a str,
    pub sni: &'a str,
    pub skip_cert_verify: bool,
    /// Same behaviour flags as the node's real kernel (`-dr`, `-udp=false`).
    pub extra_args: &'a [String],
    /// Node id that owns the temporary process (for Stop / shutdown cleanup).
    pub owner: &'a str,
    pub registry: &'a TempKernels,
}

/// Start ephemeral anytls-client, download via SOCKS, return Mbps.
pub fn measure_proxy_mbps(
    probe: &ProbeKernel<'_>,
    upstream_host: &str,
    upstream_port: u16,
    download_bytes: u64,
    overall_timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<f64, String> {
    if cancelled() {
        return Err(CANCELLED.into());
    }
    let local_port = pick_free_local_port()?;
    let listen = format!("127.0.0.1:{local_port}");
    let server = crate::ip_probe::fmt_host_port(upstream_host, upstream_port);
    let sni = if probe.sni.trim().is_empty() {
        "www.icloud.com"
    } else {
        probe.sni.trim()
    };

    let mut args: Vec<String> = vec![
        "-l".into(),
        listen.clone(),
        "-s".into(),
        server.clone(),
        "-sni".into(),
        sni.to_string(),
    ];
    if probe.skip_cert_verify {
        args.push("-insecure".into());
    }
    args.extend(probe.extra_args.iter().cloned());
    args.extend(crate::net_bind::anytls_bind_args());
    let child = Command::new(probe.kernel)
        .args(&args)
        .env("SOCKSNODE_PASSWORD", probe.password)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("临时 anytls 启动失败: {e}"))?;
    let token = probe.registry.register(probe.owner, child)?;

    let ready = wait_port_open(local_port, Duration::from_secs(3), cancelled);
    if cancelled() {
        probe.registry.reap(token);
        return Err(CANCELLED.into());
    }
    if !ready || !probe.registry.is_running(token) {
        probe.registry.reap(token);
        return Err("临时 anytls 未在 3s 内就绪".into());
    }
    // Brief settle so the first SOCKS dial does not race process startup.
    thread::sleep(Duration::from_millis(200));

    let result = measure_via_curl(local_port, download_bytes, overall_timeout, cancelled);
    probe.registry.reap(token);
    if cancelled() {
        return Err(CANCELLED.into());
    }
    result
}

/// Timeout: user setting, but enough for payload at ~8Mbps + handshake headroom.
fn effective_speed_timeout(bytes: u64, user: Duration) -> Duration {
    let need = (bytes as f64 * 8.0 / 8_000_000.0) + 6.0;
    let need = Duration::from_secs_f64(need.clamp(10.0, 60.0));
    if user > need {
        user
    } else {
        need
    }
}

/// Single-thread download via temporary SOCKS (must really traverse anytls).
///
/// LibreSpeed garbage only — **no Cloudflare** (CDN edge hides exit quality).
/// Validated: 156≈50–60 · 177≈50–56 · 103≈1–2 Mbps via real socks5h + physical bind.
///
/// NOTE: curl must use socks5h **without** `--noproxy *` (see net_bind).
fn measure_via_curl(
    socks_port: u16,
    download_bytes: u64,
    timeout: Duration,
    cancelled: &dyn Fn() -> bool,
) -> Result<f64, String> {
    let bytes = download_bytes.clamp(4 * 1024 * 1024, 50 * 1024 * 1024);
    let timeout = effective_speed_timeout(bytes, timeout);
    // LibreSpeed ckSize ≈ MiB of body.
    let ck_mb = (bytes / (1024 * 1024)).clamp(5, 30);

    // Realping: 1 MiB LibreSpeed proves tunnel is alive.
    let mut ping_ok = false;
    let ping_url =
        "https://ams.speedtest.clouvider.net/backend/garbage.php?ckSize=1".to_string();
    for _ in 0..2 {
        if cancelled() {
            return Err(CANCELLED.into());
        }
        match crate::net_bind::curl_via_socks_download(
            socks_port,
            &ping_url,
            Duration::from_secs(12),
            None,
        ) {
            Ok(s) if s.bytes >= 64 * 1024 => {
                ping_ok = true;
                break;
            }
            _ => thread::sleep(Duration::from_millis(300)),
        }
    }
    if !ping_ok {
        return Err("SOCKS 隧道不可用（LibreSpeed realping 失败）".into());
    }

    // Prefer AMS → FRA → ATL; stop at first usable sample (single-thread).
    let hosts = [
        ("AMS", "ams.speedtest.clouvider.net"),
        ("FRA", "fra.speedtest.clouvider.net"),
        ("ATL", "atl.speedtest.clouvider.net"),
    ];
    let mut errors: Vec<String> = Vec::new();
    for (tag, host) in hosts {
        if cancelled() {
            return Err(CANCELLED.into());
        }
        let url = format!("https://{host}/backend/garbage.php?ckSize={ck_mb}");
        match crate::net_bind::curl_via_socks_download(
            socks_port,
            &url,
            timeout.max(Duration::from_secs(20)),
            None,
        ) {
            Ok(s) if s.bytes >= 512 * 1024 => return Ok(s.mbps),
            Ok(s) => errors.push(format!("{tag} too-small {}B", s.bytes)),
            Err(e) => errors.push(format!("{tag}: {e}")),
        }
    }

    Err(if errors.is_empty() {
        "测速无有效样本".into()
    } else {
        errors.join("; ")
    })
}

/// Rank Top-K latency winners, optionally speed-test them, return best by strategy.
///
/// Important: under throughput/balanced, IPs that failed speed test must NOT lose to a
/// high-RTT IP solely because the latter got a non-zero Mbps while the former is 0.
/// We only compare speed among successful speed samples; otherwise fall back to RTT.
pub fn rank_and_pick(
    strategy: SelectStrategy,
    latency_results: &[ProbeResult],
    top_k: usize,
    speed_test: impl Fn(&str) -> Result<f64, String>,
    cancelled: &dyn Fn() -> bool,
) -> Result<(ScoredIp, Vec<ScoredIp>), String> {
    // Drop impossibly low / near-timeout RTTs.
    let mut reachable: Vec<(String, u64)> = latency_results
        .iter()
        .filter_map(|r| {
            let ms = r.ms?;
            if !(12..=1500).contains(&ms) {
                return None;
            }
            Some((r.ip.clone(), ms))
        })
        .collect();

    // If everything looked fake-fast, fall back to raw samples (better than nothing).
    if reachable.is_empty() {
        reachable = latency_results
            .iter()
            .filter_map(|r| r.ms.map(|ms| (r.ip.clone(), ms)))
            .collect();
    }
    if reachable.is_empty() {
        return Err("没有可达 IP".into());
    }
    reachable.sort_by_key(|(_, ms)| *ms);
    let k = top_k.clamp(1, 8).min(reachable.len());
    let shortlist: Vec<(String, u64)> = reachable[..k].to_vec();

    let mut scored = Vec::new();
    for (ip, rtt) in &shortlist {
        if cancelled() {
            return Err(CANCELLED.into());
        }
        let (mbps, note) = if matches!(strategy, SelectStrategy::Latency) {
            (None, "latency-only".into())
        } else {
            match speed_test(ip) {
                Ok(v) if v > 0.05 => (Some(v), format!("{v:.2} Mbps")),
                Ok(v) => (None, format!("speed-too-low: {v:.3} Mbps")),
                Err(e) => (None, format!("speed-fail: {e}")),
            }
        };
        scored.push(ScoredIp {
            ip: ip.clone(),
            rtt_ms: *rtt,
            mbps,
            score: 0.0,
            note,
        });
    }

    if cancelled() {
        return Err(CANCELLED.into());
    }
    let best_rtt = scored.iter().map(|s| s.rtt_ms).min().unwrap_or(1);
    let with_speed: Vec<usize> = scored
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.mbps.map(|_| i))
        .collect();

    match strategy {
        SelectStrategy::Latency => {
            for s in &mut scored {
                s.score = score_candidate(SelectStrategy::Latency, s.rtt_ms, None);
            }
        }
        SelectStrategy::Throughput | SelectStrategy::Balanced => {
            if !with_speed.is_empty() {
                // Only IPs with a successful speed sample may win.
                let speeds: Vec<f64> = scored.iter().filter_map(|s| s.mbps).collect();
                let max_s = speeds.iter().cloned().fold(0.0_f64, f64::max);
                let min_s = speeds
                    .iter()
                    .cloned()
                    .fold(f64::INFINITY, f64::min)
                    .min(max_s);
                // CDN/noise: 57 vs 46 Mbps is not real path difference. When the
                // spread is small, rank by latency so CF-equalized samples don't
                // randomly promote a worse exit.
                let bandwidth_tied = speeds.len() >= 2
                    && min_s > 0.05
                    && max_s / min_s <= 1.35
                    && (max_s - min_s) <= 25.0;

                let best_rtt_with_speed = scored
                    .iter()
                    .filter(|s| s.mbps.is_some())
                    .map(|s| s.rtt_ms)
                    .min()
                    .unwrap_or(best_rtt);

                for s in &mut scored {
                    if let Some(mbps) = s.mbps {
                        if bandwidth_tied {
                            s.score =
                                score_candidate(SelectStrategy::Latency, s.rtt_ms, Some(mbps));
                            s.note = format!("{mbps:.2} Mbps · bandwidth-tied→latency");
                        } else {
                            s.score = score_candidate(strategy, s.rtt_ms, Some(mbps));
                            if s.rtt_ms
                                > best_rtt_with_speed
                                    .saturating_mul(2)
                                    .saturating_add(40)
                            {
                                let ratio =
                                    s.rtt_ms as f64 / (best_rtt_with_speed.max(1) as f64);
                                s.score /= ratio.max(2.0);
                                s.note =
                                    format!("{} · latency-penalty(x{ratio:.1})", s.note);
                            }
                        }
                    } else {
                        s.score = -1.0;
                        s.note = format!("{} · excluded(no-speed)", s.note);
                    }
                }
            } else {
                // All speed tests failed → fall back to pure latency.
                for s in &mut scored {
                    s.score = score_candidate(SelectStrategy::Latency, s.rtt_ms, None);
                    s.note = format!("{} · fallback-latency", s.note);
                }
            }
        }
    }

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.rtt_ms.cmp(&b.rtt_ms))
    });

    let best = scored
        .first()
        .cloned()
        .ok_or_else(|| "无评分结果".to_string())?;
    Ok((best, scored))
}

/// Keep compiler happy for unused Read/Write in some toolchains.
#[allow(dead_code)]
fn _touch_io(stream: &mut TcpStream) {
    let mut buf = [0u8; 1];
    let _ = stream.read(&mut buf);
    let _ = stream.write(&buf);
}

