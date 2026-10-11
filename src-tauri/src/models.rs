use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NodeStatus {
    #[serde(rename = "stopped")]
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

impl Default for NodeStatus {
    fn default() -> Self {
        Self::Stopped
    }
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

pub fn store_from_json(raw: &str) -> Result<Vec<ProxyNode>, serde_json::Error> {
    let store: StoreFile = serde_json::from_str(raw)?;
    Ok(store
        .nodes
        .into_iter()
        .map(ProxyNode::from_stored)
        .collect())
}
