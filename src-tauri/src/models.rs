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
    pub status: NodeStatus,
    pub pid: Option<u32>,
    pub last_error: Option<String>,
    pub last_test: Option<NodeTestResult>,
    pub updated_at: Option<String>,
    pub socks: String,
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
    pub mihomo_path: String,
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
            updated_at: self.updated_at.clone(),
        }
    }

    pub fn from_stored(s: StoredNode) -> Self {
        let socks = format!("socks5://127.0.0.1:{}", s.local_port);
        Self {
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
            status: NodeStatus::Stopped,
            pid: None,
            last_error: None,
            last_test: None,
            updated_at: s.updated_at,
            socks,
            running: false,
        }
    }

    pub fn refresh_public_fields(&mut self) {
        self.socks = format!("socks5://127.0.0.1:{}", self.local_port);
        self.running = self.status == NodeStatus::Running;
    }
}

pub fn default_nodes() -> Vec<ProxyNode> {
    vec![
        ProxyNode {
            id: "hk-01".into(),
            name: "香港".into(),
            proxy_type: "anytls".into(),
            server: "156.239.13.236".into(),
            port: 12002,
            password: "bfab1b89-b4db-44b8-a518-e13eb05f2cad".into(),
            udp: true,
            sni: "www.icloud.com".into(),
            skip_cert_verify: true,
            local_port: 5001,
            enabled: true,
            status: NodeStatus::Stopped,
            pid: None,
            last_error: None,
            last_test: None,
            updated_at: Some(now_iso()),
            socks: "socks5://127.0.0.1:5001".into(),
            running: false,
        },
        ProxyNode {
            id: "us-bgp".into(),
            name: "美国".into(),
            proxy_type: "anytls".into(),
            server: "znjxdm.lajiaoyun.sbs".into(),
            port: 50001,
            password: "bfab1b89-b4db-44b8-a518-e13eb05f2cad".into(),
            udp: true,
            sni: "www.icloud.com".into(),
            skip_cert_verify: true,
            local_port: 5002,
            enabled: true,
            status: NodeStatus::Stopped,
            pid: None,
            last_error: None,
            last_test: None,
            updated_at: Some(now_iso()),
            socks: "socks5://127.0.0.1:5002".into(),
            running: false,
        },
    ]
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
