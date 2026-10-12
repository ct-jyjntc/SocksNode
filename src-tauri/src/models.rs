use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum NodeStatus {
    #[serde(rename = "stopped")]
    #[default]
    Stopped,
    #[serde(rename = "starting")]
    Starting,
    #[serde(rename = "running")]
    Running,
    #[serde(rename = "stopping")]
    Stopping,
    #[serde(rename = "error")]
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTestResult {
    pub ok: bool,
    pub ms: u64,
    pub ip: Option<String>,
    pub message: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeStatus {
    pub ok: bool,
    pub ms: Option<u64>,
    pub ip: Option<String>,
    pub message: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyNode {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub proxy_type: String,
    pub server: String,
    pub port: u16,
    pub password: String,
    pub udp: bool,
    pub sni: String,
    pub skip_cert_verify: bool,
    /// Enable AnyTLS client session reuse. Default off: kernel runs with -dr
    /// (every connection dials fresh) unless the user opts in.
    #[serde(default)]
    pub session_reuse: bool,
    pub local_port: u16,
    pub enabled: bool,
    /// Prefer multi-source DNS + local tcping for domain hosts.
    #[serde(default = "default_true")]
    pub prefer_fastest_ip: bool,
    /// Bind SOCKS on 0.0.0.0 so LAN devices can connect.
    #[serde(default)]
    pub lan_access: bool,
    /// Currently selected upstream IP (runtime; may differ from `server` domain).
    #[serde(default)]
    pub active_ip: Option<String>,
    #[serde(default)]
    pub last_probe: Option<ProbeStatus>,
    pub status: NodeStatus,
    pub pid: Option<u32>,
    pub last_error: Option<String>,
    pub last_test: Option<NodeTestResult>,
    pub updated_at: Option<String>,
    /// Local-only SOCKS URL (127.0.0.1).
    pub socks: String,
    /// LAN-reachable SOCKS URL when `lan_access` is on (best-effort primary IPv4).
    #[serde(default)]
    pub lan_socks: Option<String>,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInput {
    pub name: String,
    #[serde(default = "default_type", rename = "type")]
    pub proxy_type: String,
    pub server: String,
    pub port: u16,
    pub password: String,
    #[serde(default = "default_true")]
    pub udp: bool,
    #[serde(default = "default_sni")]
    pub sni: String,
    #[serde(default)]
    pub skip_cert_verify: bool,
    #[serde(default)]
    pub session_reuse: bool,
    pub local_port: u16,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub prefer_fastest_ip: bool,
    #[serde(default)]
    pub lan_access: bool,
}

fn default_type() -> String {
    "anytls".into()
}
fn default_sni() -> String {
    "www.icloud.com".into()
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub data_dir: String,
    pub kernel_path: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub ts: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub id: String,
    pub ts: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredNode {
    id: String,
    name: String,
    #[serde(rename = "type")]
    proxy_type: String,
    server: String,
    port: u16,
    password: String,
    udp: bool,
    sni: String,
    #[serde(rename = "skipCertVerify", alias = "skip_cert_verify")]
    skip_cert_verify: bool,
    #[serde(rename = "sessionReuse", alias = "session_reuse", default)]
    session_reuse: bool,
    #[serde(rename = "localPort", alias = "local_port")]
    local_port: u16,
    enabled: bool,
    #[serde(
        rename = "preferFastestIp",
        alias = "prefer_fastest_ip",
        default = "default_true"
    )]
    prefer_fastest_ip: bool,
    #[serde(rename = "lanAccess", alias = "lan_access", default)]
    lan_access: bool,
    /// Sticky preferred upstream IP after a successful select (skip re-test while alive).
    #[serde(rename = "activeIp", alias = "active_ip", default)]
    active_ip: Option<String>,
    #[serde(rename = "updatedAt", alias = "updated_at", default)]
    updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    #[serde(rename = "updatedAt", alias = "updated_at")]
    updated_at: String,
    nodes: Vec<StoredNode>,
}

impl ProxyNode {
    pub fn to_stored(&self) -> StoredNode {
        StoredNode {
            id: self.id.clone(),
            name: self.name.clone(),
            proxy_type: self.proxy_type.clone(),
            server: self.server.clone(),
            port: self.port,
            password: self.password.clone(),
            udp: self.udp,
            sni: self.sni.clone(),
            skip_cert_verify: self.skip_cert_verify,
            session_reuse: self.session_reuse,
            local_port: self.local_port,
            enabled: self.enabled,
            prefer_fastest_ip: self.prefer_fastest_ip,
            lan_access: self.lan_access,
            active_ip: self.active_ip.clone(),
            updated_at: self.updated_at.clone(),
        }
    }

    pub fn from_stored(s: StoredNode) -> Self {
        let mut node = Self {
            id: s.id,
            name: s.name,
            proxy_type: s.proxy_type,
            server: s.server,
            port: s.port,
            password: s.password,
            udp: s.udp,
            sni: s.sni,
            skip_cert_verify: s.skip_cert_verify,
            session_reuse: s.session_reuse,
            local_port: s.local_port,
            enabled: s.enabled,
            prefer_fastest_ip: s.prefer_fastest_ip,
            lan_access: s.lan_access,
            active_ip: s.active_ip,
            last_probe: None,
            status: NodeStatus::Stopped,
            pid: None,
            last_error: None,
            last_test: None,
            updated_at: s.updated_at,
            socks: String::new(),
            lan_socks: None,
            running: false,
        };
        node.refresh_public_fields();
        node
    }

    pub fn refresh_public_fields(&mut self) {
        self.socks = format!("socks5://127.0.0.1:{}", self.local_port);
        self.lan_socks = if self.lan_access {
            primary_lan_ipv4().map(|ip| format!("socks5://{ip}:{}", self.local_port))
        } else {
            None
        };
        self.running = self.status == NodeStatus::Running;
    }

    pub fn listen_addr(&self) -> String {
        if self.lan_access {
            format!("0.0.0.0:{}", self.local_port)
        } else {
            format!("127.0.0.1:{}", self.local_port)
        }
    }
}

/// Best-effort primary LAN IPv4 (outbound interface).
pub fn primary_lan_ipv4() -> Option<String> {
    use std::net::{IpAddr, UdpSocket};
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    // No packets need to be sent; connect selects the outbound interface.
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_unspecified() => Some(v4.to_string()),
        _ => None,
    }
}

/// No built-in nodes: the app never ships credentials or preset servers.
/// Users add their own nodes on first run; nothing sensitive lives in the repo.
pub fn default_nodes() -> Vec<ProxyNode> {
    Vec::new()
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn store_to_json(nodes: &[ProxyNode]) -> Result<String, serde_json::Error> {
    let store = StoreFile {
        version: 1,
        updated_at: now_iso(),
        nodes: nodes.iter().map(|n| n.to_stored()).collect(),
    };
    serde_json::to_string_pretty(&store)
}

/// Ids written before the UUID hardening (e.g. "hk-01") are safe directory
/// names but fail strict UUID parsing. Derive a stable UUID for them so old
/// configs keep loading; ids outside `[A-Za-z0-9_-]` stay rejected.
pub fn migrate_legacy_id(id: &str) -> Option<String> {
    let safe = !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !safe || uuid::Uuid::parse_str(id).is_ok() {
        return None;
    }
    // Deterministic RFC 4122-layout UUID via FNV-1a over a namespaced input,
    // so every launch and the instance-dir rename agree on the same new id.
    fn fnv1a(seed: u64, data: &[u8]) -> u64 {
        let mut h = seed;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    let input = format!("socksnode-legacy-id-v1:{id}");
    let hi = fnv1a(OFFSET, input.as_bytes());
    let lo = fnv1a(OFFSET ^ 0x9e37_79b9_7f4a_7c15, input.as_bytes());
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&hi.to_be_bytes());
    bytes[8..].copy_from_slice(&lo.to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant RFC 4122
    Some(uuid::Uuid::from_bytes(bytes).to_string())
}

/// (old_id, new_id) pairs in a stored config that need legacy id migration.
pub fn legacy_id_map(raw: &str) -> Vec<(String, String)> {
    let Ok(store) = serde_json::from_str::<StoreFile>(raw) else {
        return Vec::new();
    };
    store
        .nodes
        .iter()
        .filter_map(|n| migrate_legacy_id(&n.id).map(|new| (n.id.clone(), new)))
        .collect()
}

pub fn store_from_json(raw: &str) -> Result<Vec<ProxyNode>, serde_json::Error> {
    let mut store: StoreFile = serde_json::from_str(raw)?;
    let mut seen = std::collections::HashSet::new();
    for node in &mut store.nodes {
        // Node ids are used as directory names and receive untrusted IPC input.
        // Reject path traversal, invalid ids, and duplicates in hand-edited files.
        if uuid::Uuid::parse_str(&node.id).is_err() {
            match migrate_legacy_id(&node.id) {
                Some(new_id) => node.id = new_id,
                None => {
                    return Err(serde_json::Error::io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "节点配置包含非法或重复 ID",
                    )))
                }
            }
        }
        if !seen.insert(node.id.clone()) {
            return Err(serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "节点配置包含非法或重复 ID",
            )));
        }
    }
    Ok(store
        .nodes
        .into_iter()
        .map(ProxyNode::from_stored)
        .collect())
}
