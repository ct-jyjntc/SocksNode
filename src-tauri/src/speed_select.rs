use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
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
fn wait_port_open(port: u16, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
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

fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Start ephemeral anytls-client, download via SOCKS, return Mbps.
pub fn measure_proxy_mbps(
    kernel: &Path,
    upstream_host: &str,
    upstream_port: u16,
    password: &str,
    sni: &str,
    download_bytes: u64,
    overall_timeout: Duration,
) -> Result<f64, String> {
    let local_port = pick_free_local_port()?;
    let listen = format!("127.0.0.1:{local_port}");
    let server = format!("{upstream_host}:{upstream_port}");
    let sni = if sni.trim().is_empty() {
        "www.icloud.com"
    } else {
        sni.trim()
    };

    let mut args: Vec<String> = vec![
        "-l".into(),
        listen.clone(),
        "-s".into(),
        server.clone(),
        "-p".into(),
        password.to_string(),
        "-sni".into(),
        sni.to_string(),
    ];
    args.extend(crate::net_bind::anytls_bind_args());
    let mut child = Command::new(kernel)
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("临时 anytls 启动失败: {e}"))?;

    if !wait_port_open(local_port, Duration::from_secs(3)) {
        kill_child(&mut child);
        return Err("临时 anytls 未在 3s 内就绪".into());
    }
    // Brief settle so the first SOCKS dial does not race process startup.
    thread::sleep(Duration::from_millis(200));

    let result = measure_via_curl(local_port, download_bytes, overall_timeout);
    kill_child(&mut child);
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
fn measure_via_curl(socks_port: u16, download_bytes: u64, timeout: Duration) -> Result<f64, String> {
    let bytes = download_bytes.clamp(4 * 1024 * 1024, 50 * 1024 * 1024);
    let timeout = effective_speed_timeout(bytes, timeout);
    // LibreSpeed ckSize ≈ MiB of body.
    let ck_mb = (bytes / (1024 * 1024)).clamp(5, 30);

    // Realping: 1 MiB LibreSpeed proves tunnel is alive.
    let mut ping_ok = false;
    let ping_url =
        "https://ams.speedtest.clouvider.net/backend/garbage.php?ckSize=1".to_string();
    for _ in 0..2 {
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
) -> Result<(ScoredIp, Vec<ScoredIp>), String> {
    // Drop impossibly low / near-timeout RTTs.
    let mut reachable: Vec<(String, u64)> = latency_results
        .iter()
        .filter_map(|r| {
            let ms = r.ms?;
            if ms < 12 || ms > 1500 {
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

