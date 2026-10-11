use crate::ilatency;
use crate::ip_probe::{
    is_domain_host, merge_unique, probe_all, resolve_doh, resolve_system, tcping_with_retries,
};
use crate::net_bind;
use crate::models::{
    default_nodes, now_iso, store_from_json, store_to_json, AppInfo, LogEntry, LogEvent, NodeInput,
    NodeStatus, NodeTestResult, ProbeStatus, ProxyNode,
};
use crate::settings::{AppSettings, AppSettingsPatch, AppSettingsView};
use crate::speed_select::{
    measure_proxy_mbps, rank_and_pick, SelectStrategy,
};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager as TauriManager};

type SharedLogs = Arc<Mutex<HashMap<String, Vec<LogEntry>>>>;
type SharedProcs = Arc<Mutex<HashMap<String, Child>>>;
type SharedNodes = Arc<Mutex<Vec<ProxyNode>>>;
type SharedSettings = Arc<Mutex<AppSettings>>;
type SharedFailoverAt = Arc<Mutex<HashMap<String, Instant>>>;
type SharedGenerations = Arc<Mutex<HashMap<String, u64>>>;
/// Per-node transition locks: serialize process creation, status update and stopping.
type SharedLifecycleGates = Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>;
type SharedDnsCache = Arc<Mutex<HashMap<String, (Instant, Vec<String>)>>>;

const TCPING_TIMEOUT: Duration = Duration::from_secs(2);
const TCPING_RETRIES: u32 = 3;
const TCPING_RETRY_GAP: Duration = Duration::from_secs(1);

/// Prevent concurrent start of the same node (map insert would orphan the first Child).
struct StartGuard {
    set: Arc<Mutex<HashSet<String>>>,
    id: String,
}

impl Drop for StartGuard {
    fn drop(&mut self) {
        self.set.lock().remove(&self.id);
    }
}

fn force_kill_pid(pid: u32) {
    if pid == 0 {
        return;
    }
    let _ = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn port_is_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
        && TcpListener::bind(("0.0.0.0", port)).is_ok()
}

/// True only when pid is one of our bundled anytls-client processes.
///
/// Every forced kill goes through this guard: the local SOCKS ports are
/// user-configurable, so we must never kill an unrelated service that
/// happens to listen on the same port.
fn pid_is_our_kernel(pid: u32, kernel: &std::path::Path) -> bool {
    if pid == 0 {
        return false;
    }
    let kernel = kernel.to_string_lossy();
    if kernel.trim().is_empty() {
        return false;
    }
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "command="])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).contains(kernel.as_ref())
        }
        _ => false,
    }
}

/// Kill LISTENers on port only if they are our own anytls-client.
/// Foreign listeners are left alone; callers surface a port-conflict error.
fn kill_owned_listeners_on_port(port: u16, kernel: &std::path::Path) {
    let port = port.to_string();
    let tcp_filter = ["-iTCP:", &port].concat();
    let output = Command::new("lsof")
        .args([
            "-nP",
            tcp_filter.as_str(),
            "-sTCP:LISTEN",
            "-t",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    let Ok(out) = output else {
        return;
    };
    for pid in String::from_utf8_lossy(&out.stdout).split_whitespace() {
        if let Ok(p) = pid.parse::<u32>() {
            if pid_is_our_kernel(p, kernel) {
                force_kill_pid(p);
            }
        }
    }
}

fn kill_child_process(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

pub struct Manager {
    data_dir: PathBuf,
    resources_dir: PathBuf,
    store_path: PathBuf,
    instances_dir: PathBuf,
    nodes: SharedNodes,
    processes: SharedProcs,
    logs: SharedLogs,
    settings: SharedSettings,
    last_failover_at: SharedFailoverAt,
    /// host -> (cached_at, ips); short TTL for multi-location DNS.
    dns_cache: SharedDnsCache,
    /// Node ids currently inside `start()` — blocks double-start races.
    starting_ids: Arc<Mutex<HashSet<String>>>,
    /// Per-node watcher generation; bumped on every start/stop so stale
    /// health/exit watchers terminate instead of piling up.
    generations: SharedGenerations,
    lifecycle_gates: SharedLifecycleGates,
    is_shutting_down: Arc<Mutex<bool>>,
    app: Mutex<Option<AppHandle>>,
}

impl Manager {
    pub fn new(data_dir: PathBuf, resources_dir: PathBuf) -> Self {
        let store_path = data_dir.join("nodes.json");
        let instances_dir = data_dir.join("instances");
        let settings = AppSettings::load(&data_dir);
        Self {
            data_dir,
            resources_dir,
            store_path,
            instances_dir,
            nodes: Arc::new(Mutex::new(Vec::new())),
            processes: Arc::new(Mutex::new(HashMap::new())),
            logs: Arc::new(Mutex::new(HashMap::new())),
            settings: Arc::new(Mutex::new(settings)),
            last_failover_at: Arc::new(Mutex::new(HashMap::new())),
            dns_cache: Arc::new(Mutex::new(HashMap::new())),
            starting_ids: Arc::new(Mutex::new(HashSet::new())),
            generations: Arc::new(Mutex::new(HashMap::new())),
            lifecycle_gates: Arc::new(Mutex::new(HashMap::new())),
            is_shutting_down: Arc::new(Mutex::new(false)),
            app: Mutex::new(None),
        }
    }

    pub fn get_settings_view(&self, launch_at_login_active: bool) -> AppSettingsView {
        let settings = self.settings.lock().clone();
        let user = if settings.ilatency_cookie.trim().is_empty() {
            None
        } else {
            ilatency::session_username(&settings.ilatency_cookie).ok()
        };
        settings.to_view(user, launch_at_login_active)
    }

    pub fn update_settings(
        &self,
        patch: AppSettingsPatch,
        launch_at_login_active: bool,
    ) -> Result<AppSettingsView, String> {
        {
            let mut settings = self.settings.lock();
            if let Some(cookie) = patch.ilatency_cookie {
                settings.ilatency_cookie = cookie.trim().to_string();
            }
            if let Some(v) = patch.health_interval_secs {
                settings.health_interval_secs = v.clamp(3, 120);
            }
            if let Some(v) = patch.failover_cooldown_secs {
                settings.failover_cooldown_secs = v.clamp(15, 600);
            }
            if let Some(s) = patch.select_strategy {
                settings.select_strategy = SelectStrategy::parse(&s);
            }
            if let Some(v) = patch.speed_top_k {
                settings.speed_top_k = v.clamp(1, 8);
            }
            if let Some(v) = patch.speed_test_bytes {
                settings.speed_test_bytes = v.clamp(1024 * 1024, 50 * 1024 * 1024);
            }
            if let Some(v) = patch.speed_test_timeout_secs {
                settings.speed_test_timeout_secs = v.clamp(10, 60);
            }
            if let Some(v) = patch.launch_at_login {
                settings.launch_at_login = v;
            }
            if let Some(v) = patch.start_minimized {
                settings.start_minimized = v;
            }
            if let Some(v) = patch.auto_start_nodes {
                settings.auto_start_nodes = v;
            }
            // Minimized login start only makes sense with launch-at-login.
            if !settings.launch_at_login {
                settings.start_minimized = false;
            }
            settings.save(&self.data_dir)?;
        }
        Ok(self.get_settings_view(launch_at_login_active))
    }

    pub fn launch_at_login_setting(&self) -> bool {
        self.settings.lock().launch_at_login
    }

    pub fn start_minimized_setting(&self) -> bool {
        let s = self.settings.lock();
        s.launch_at_login && s.start_minimized
    }

    pub fn auto_start_nodes_setting(&self) -> bool {
        self.settings.lock().auto_start_nodes
    }

    pub fn test_ilatency_session(&self) -> Result<String, String> {
        let cookie = self.settings.lock().ilatency_cookie.clone();
        ilatency::session_username(&cookie)
    }

    pub fn set_app(&self, app: AppHandle) {
        *self.app.lock() = Some(app);
    }

    pub fn init(&self) -> Result<(), String> {
        std::fs::create_dir_all(&self.instances_dir).map_err(|e| e.to_string())?;
        let mut nodes = if self.store_path.exists() {
            let raw = std::fs::read_to_string(&self.store_path).map_err(|e| e.to_string())?;
            store_from_json(&raw).map_err(|e| e.to_string())?
        } else {
            let defaults = default_nodes();
            self.persist_nodes(&defaults)?;
            defaults
        };

        // Previous app sessions / crashed starts can leave anytls-client orphans
        // holding local SOCKS ports — free them before anything starts.
        self.kill_stale_anytls_clients();
        let kernel = self.kernel_path();
        for n in &nodes {
            if !port_is_free(n.local_port) {
                kill_owned_listeners_on_port(n.local_port, &kernel);
            }
        }
        thread::sleep(Duration::from_millis(80));

        let mut logs = self.logs.lock();
        for n in &mut nodes {
            n.status = NodeStatus::Stopped;
            n.pid = None;
            n.last_error = None;
            n.refresh_public_fields();
            logs.entry(n.id.clone()).or_default();
        }
        drop(logs);
        *self.nodes.lock() = nodes;
        Ok(())
    }

    fn kill_stale_anytls_clients(&self) {
        let kernel = self.kernel_path();
        let path = kernel.to_string_lossy();
        if path.is_empty() {
            return;
        }
        // Match our bundled binary path so we don't kill unrelated processes.
        let _ = Command::new("pkill")
            .args(["-9", "-f", path.as_ref()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }

    /// Kill managed child + recorded pid, and free the local SOCKS port if needed.
    fn reclaim_node_runtime(&self, id: &str, local_port: u16) {
        if let Some(mut child) = self.processes.lock().remove(id) {
            kill_child_process(&mut child);
        }
        let kernel = self.kernel_path();
        let pid = {
            let nodes = self.nodes.lock();
            nodes.iter().find(|n| n.id == id).and_then(|n| n.pid)
        };
        if let Some(pid) = pid {
            if pid_is_our_kernel(pid, &kernel) {
                force_kill_pid(pid);
            }
        }
        if !port_is_free(local_port) {
            kill_owned_listeners_on_port(local_port, &kernel);
            // TIME_WAIT / kernel release
            for _ in 0..10 {
                thread::sleep(Duration::from_millis(40));
                if port_is_free(local_port) {
                    break;
                }
            }
        }
    }

    /// Turn a failed start into a visible Error state instead of leaving the
    /// node stuck in `Starting` forever.
    fn abort_start(&self, id: &str, msg: String, silent: bool) -> Result<Vec<ProxyNode>, String> {
        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.status = NodeStatus::Error;
                node.pid = None;
                node.last_error = Some(msg.clone());
            }
        }
        self.append_log(id, "start failed");
        self.append_log(id, &msg);
        self.emit_nodes();
        if silent {
            Ok(self.list())
        } else {
            Err(msg)
        }
    }

    /// Invalidate every watcher previously spawned for this node.
    fn bump_generation(&self, id: &str) -> u64 {
        let mut map = self.generations.lock();
        let entry = map.entry(id.to_string()).or_insert(0);
        *entry = entry.wrapping_add(1);
        *entry
    }

    /// Only the transition phase is locked, never network discovery or speed tests.
    fn lifecycle_gate(&self, id: &str) -> Arc<Mutex<()>> {
        Arc::clone(
            self.lifecycle_gates
                .lock()
                .entry(id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    fn generation_is_current(&self, id: &str, generation: u64) -> bool {
        self.generations.lock().get(id).copied() == Some(generation)
    }

    fn cancelled_start(&self, silent: bool) -> Result<Vec<ProxyNode>, String> {
        if silent {
            Ok(self.list())
        } else {
            Err("节点启动已取消".into())
        }
    }

    fn begin_start_guard(&self, id: &str) -> Option<StartGuard> {
        let mut set = self.starting_ids.lock();
        if !set.insert(id.to_string()) {
            return None;
        }
        Some(StartGuard {
            set: Arc::clone(&self.starting_ids),
            id: id.to_string(),
        })
    }

    fn persist_nodes(&self, nodes: &[ProxyNode]) -> Result<(), String> {
        let json = store_to_json(nodes).map_err(|e| e.to_string())?;
        crate::secure_store::write_private(&self.store_path, json.as_bytes())
    }

    fn persist(&self) -> Result<(), String> {
        let nodes = self.nodes.lock();
        self.persist_nodes(&nodes)
    }

    fn emit_nodes(&self) {
        let list = self.list();
        if let Some(app) = self.app.lock().as_ref() {
            let _ = app.emit("nodes:changed", list);
        }
    }

    fn emit_nodes_shared(
        nodes: &SharedNodes,
        app: &Option<AppHandle>,
    ) {
        let mut list = nodes.lock().clone();
        for n in &mut list {
            n.refresh_public_fields();
        }
        if let Some(app) = app {
            let _ = app.emit("nodes:changed", list);
        }
    }

    pub fn list(&self) -> Vec<ProxyNode> {
        let mut nodes = self.nodes.lock().clone();
        for n in &mut nodes {
            n.refresh_public_fields();
        }
        nodes
    }

    pub fn get_logs(&self, id: &str) -> Vec<LogEntry> {
        self.logs.lock().get(id).cloned().unwrap_or_default()
    }

    pub fn get_info(&self, version: &str) -> AppInfo {
        AppInfo {
            data_dir: self.data_dir.display().to_string(),
            kernel_path: self.kernel_path().display().to_string(),
            version: version.to_string(),
        }
    }

    pub fn kernel_path(&self) -> PathBuf {
        let candidates = [
            self.resources_dir.join("anytls-client"),
            self.resources_dir.join("resources").join("anytls-client"),
        ];
        for p in candidates {
            if p.exists() {
                return p;
            }
        }
        self.resources_dir.join("anytls-client")
    }

    fn append_log(&self, id: &str, line: impl AsRef<str>) {
        let entry = LogEntry {
            ts: now_iso(),
            line: line.as_ref().trim_end().to_string(),
        };
        // Always print to terminal in dev so `npm run dev` shows selection progress.
        eprintln!("[socks-node:{id}] {}", entry.line);
        {
            let mut logs = self.logs.lock();
            let list = logs.entry(id.to_string()).or_default();
            list.push(entry.clone());
            while list.len() > 500 {
                list.remove(0);
            }
        }
        // Persist for later inspection (UI + after restart).
        let log_path = self.instances_dir.join(id).join("runtime.log");
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let _ = writeln!(f, "{} {}", entry.ts, entry.line);
        }
        if let Some(app) = self.app.lock().as_ref() {
            let _ = app.emit(
                "nodes:log",
                LogEvent {
                    id: id.to_string(),
                    ts: entry.ts.clone(),
                    line: entry.line.clone(),
                },
            );
        }
    }

    fn instance_dir(&self, id: &str) -> Result<PathBuf, String> {
        let dir = self.instances_dir.join(id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir)
    }

    fn normalize_input(
        &self,
        input: &NodeInput,
        existing_id: Option<&str>,
    ) -> Result<NodeInput, String> {
        let name = input.name.trim().to_string();
        let server = input.server.trim().to_string();
        let password = input.password.trim().to_string();
        if name.is_empty() {
            return Err("节点名称不能为空".into());
        }
        if server.is_empty() {
            return Err("服务器地址不能为空".into());
        }
        if password.is_empty() {
            return Err("密码不能为空".into());
        }
        if input.port == 0 {
            return Err("远程端口无效".into());
        }
        if input.local_port == 0 {
            return Err("本地 SOCKS 端口无效".into());
        }

        let nodes = self.nodes.lock();
        if nodes
            .iter()
            .any(|n| Some(n.id.as_str()) != existing_id && n.local_port == input.local_port)
        {
            return Err(format!("本地端口 {} 已被其他节点使用", input.local_port));
        }

        Ok(NodeInput {
            name,
            proxy_type: if input.proxy_type.trim().is_empty() {
                "anytls".into()
            } else {
                input.proxy_type.trim().to_string()
            },
            server,
            port: input.port,
            password,
            udp: input.udp,
            sni: {
                let s = input.sni.trim().to_string();
                if s.is_empty() {
                    "www.icloud.com".into()
                } else {
                    s
                }
            },
            skip_cert_verify: input.skip_cert_verify,
            local_port: input.local_port,
            enabled: input.enabled,
            prefer_fastest_ip: input.prefer_fastest_ip,
            lan_access: input.lan_access,
        })
    }

    pub fn create(&self, input: NodeInput) -> Result<Vec<ProxyNode>, String> {
        let data = self.normalize_input(&input, None)?;
        let mut node = ProxyNode {
            id: uuid::Uuid::new_v4().to_string(),
            name: data.name,
            proxy_type: data.proxy_type,
            server: data.server,
            port: data.port,
            password: data.password,
            udp: data.udp,
            sni: data.sni,
            skip_cert_verify: data.skip_cert_verify,
            local_port: data.local_port,
            enabled: data.enabled,
            prefer_fastest_ip: data.prefer_fastest_ip,
            lan_access: data.lan_access,
            active_ip: None,
            last_probe: None,
            status: NodeStatus::Stopped,
            pid: None,
            last_error: None,
            last_test: None,
            updated_at: Some(now_iso()),
            socks: String::new(),
            lan_socks: None,
            running: false,
        };
        node.refresh_public_fields();
        self.logs.lock().insert(node.id.clone(), Vec::new());
        {
            let mut nodes = self.nodes.lock();
            nodes.insert(0, node);
        }
        self.persist()?;
        self.emit_nodes();
        Ok(self.list())
    }

    pub fn update(&self, id: &str, patch: NodeInput) -> Result<Vec<ProxyNode>, String> {
        // Validate before touching a live process. Invalid edits must not cause downtime.
        let data = self.normalize_input(&patch, Some(id))?;
        let (was_running, was_starting) = {
            let nodes = self.nodes.lock();
            let node = nodes.iter().find(|n| n.id == id).ok_or("节点不存在")?;
            (
                node.status == NodeStatus::Running,
                node.status == NodeStatus::Starting,
            )
        };
        if was_running || was_starting {
            self.stop(id, true)?;
        }
        {
            let mut nodes = self.nodes.lock();
            let node = nodes.iter_mut().find(|n| n.id == id).ok_or("节点不存在")?;
            // Drop sticky IP only when the upstream identity changes.
            let upstream_changed =
                node.server != data.server || node.port != data.port;
            node.name = data.name;
            node.proxy_type = data.proxy_type;
            node.server = data.server;
            node.port = data.port;
            node.password = data.password;
            node.udp = data.udp;
            node.sni = data.sni;
            node.skip_cert_verify = data.skip_cert_verify;
            node.local_port = data.local_port;
            node.enabled = data.enabled;
            node.prefer_fastest_ip = data.prefer_fastest_ip;
            node.lan_access = data.lan_access;
            if upstream_changed {
                node.active_ip = None;
                node.last_probe = None;
            }
            node.updated_at = Some(now_iso());
            node.last_error = None;
            node.refresh_public_fields();
        }
        self.persist()?;

        if was_running {
            let _ = self.start(id, true);
        } else {
            self.emit_nodes();
        }
        Ok(self.list())
    }

    pub fn remove(&self, id: &str) -> Result<Vec<ProxyNode>, String> {
        let running = {
            let nodes = self.nodes.lock();
            nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| matches!(n.status, NodeStatus::Running | NodeStatus::Starting))
                .ok_or_else(|| "节点不存在".to_string())?
        };
        if running {
            let _ = self.stop(id, true);
        }
        {
            let mut nodes = self.nodes.lock();
            nodes.retain(|n| n.id != id);
        }
        self.logs.lock().remove(id);
        self.generations.lock().remove(id);
        self.persist()?;
        let dir = self.instances_dir.join(id);
        let _ = std::fs::remove_dir_all(dir);
        self.emit_nodes();
        Ok(self.list())
    }

    fn assert_port_available(&self, port: u16, exclude_id: &str) -> Result<(), String> {
        let nodes = self.nodes.lock();
        if let Some(conflict) = nodes.iter().find(|n| {
            n.id != exclude_id
                && n.local_port == port
                && matches!(
                    n.status,
                    NodeStatus::Running | NodeStatus::Starting | NodeStatus::Stopping
                )
        }) {
            return Err(format!(
                "本地端口 {} 已被节点「{}」占用",
                port, conflict.name
            ));
        }
        drop(nodes);
        if !port_is_free(port) {
            return Err(format!(
                "本地端口 {} 仍被占用（可能有残留 anytls-client）",
                port
            ));
        }
        Ok(())
    }

    pub fn start(&self, id: &str, silent: bool) -> Result<Vec<ProxyNode>, String> {
        let status = {
            let nodes = self.nodes.lock();
            nodes.iter().find(|n| n.id == id).ok_or("节点不存在")?.status.clone()
        };
        // Never nest nodes -> processes locks: exit handling uses the reverse order.
        if status == NodeStatus::Running && self.processes.lock().contains_key(id) {
            return Ok(self.list());
        }

        let _guard = match self.begin_start_guard(id) {
            Some(g) => g,
            None => {
                if silent {
                    return Ok(self.list());
                }
                return Err("节点正在启动中".into());
            }
        };

        // Allocate a new generation while holding the Stop transition gate.
        let transition_gate = self.lifecycle_gate(id);
        let _transition = transition_gate.lock();
        let generation = self.bump_generation(id);

        let kernel = self.kernel_path();
        if !kernel.exists() {
            return self.abort_start(id, ["找不到 anytls 内核: ", kernel.display().to_string().as_str()].concat(), silent);
        }

        let (local_port, name_for_log) = {
            let nodes = self.nodes.lock();
            let node = nodes.iter().find(|n| n.id == id).ok_or("节点不存在")?;
            (node.local_port, node.name.clone())
        };

        // Always reclaim first — prevents "address already in use" from orphans
        // when map insert previously dropped a live Child without kill.
        self.reclaim_node_runtime(id, local_port);

        if let Err(e) = self.assert_port_available(local_port, id) {
            // One more aggressive reclaim then recheck.
            kill_owned_listeners_on_port(local_port, &kernel);
            thread::sleep(Duration::from_millis(100));
            return self.abort_start(id, e, silent);
        }

        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.status = NodeStatus::Starting;
                node.pid = None;
                node.last_error = None;
                node.last_probe = Some(ProbeStatus {
                    ok: true,
                    ms: None,
                    ip: node.active_ip.clone(),
                    message: if node.prefer_fastest_ip {
                        if node.active_ip.is_some() {
                            "检查已选 IP（存活则跳过重测）…".into()
                        } else {
                            "正在优选 IP / 测速…".into()
                        }
                    } else {
                        "正在启动…".into()
                    },
                    at: now_iso(),
                });
            }
        }
        self.emit_nodes();

        let node_snapshot = {
            let nodes = self.nodes.lock();
            nodes
                .iter()
                .find(|n| n.id == id)
                .cloned()
                .ok_or("节点不存在")?
        };

        let dir = match self.instance_dir(&node_snapshot.id) {
            Ok(dir) => dir,
            Err(e) => return self.abort_start(id, e, silent),
        };
        let listen = node_snapshot.listen_addr();
        // Allow Stop to cancel a potentially slow DNS / speed-selection phase.
        drop(_transition);
        let selected = self.select_upstream_ip(&node_snapshot);
        let _transition = transition_gate.lock();
        if !self.generation_is_current(id, generation) {
            return self.cancelled_start(silent);
        }
        let upstream_host = match selected {
            Ok(ip) => ip,
            Err(e) => return self.abort_start(id, ["IP 优选失败: ", e.as_str()].concat(), silent),
        };
        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.active_ip = Some(upstream_host.clone());
            }
        }
        let server = format!("{}:{}", upstream_host, node_snapshot.port);
        let sni = if node_snapshot.sni.is_empty() {
            "www.icloud.com".to_string()
        } else {
            node_snapshot.sni.clone()
        };

        self.append_log(
            id,
            format!("starting anytls-client on socks {listen}"),
        );
        if is_domain_host(&node_snapshot.server) {
            self.append_log(
                id,
                format!(
                    "upstream: {} → {} sni={sni}",
                    node_snapshot.server, server
                ),
            );
        } else {
            self.append_log(id, format!("upstream: {server} sni={sni}"));
        }

        let bind_args = net_bind::anytls_bind_args();
        if !bind_args.is_empty() {
            self.append_log(id, format!("anytls physical bind: {}", bind_args.join(" ")));
        }
        let mut args: Vec<String> = vec![
            "-l".into(),
            listen.clone(),
            "-s".into(),
            server.clone(),
            "-sni".into(),
            sni.clone(),
        ];
        if node_snapshot.skip_cert_verify {
            args.push("-insecure".into());
        }
        args.extend(bind_args);
        let mut child = match Command::new(&kernel)
            .args(&args)
            .env("SOCKSNODE_PASSWORD", &node_snapshot.password)
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => return self.abort_start(id, e.to_string(), silent),
        };

        let pid = child.id();
        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.pid = Some(pid);
            }
        }

        let logs = Arc::clone(&self.logs);
        let app_handle = self.app.lock().clone();
        if let Some(stdout) = child.stdout.take() {
            let id_c = id.to_string();
            let logs_c = Arc::clone(&logs);
            let app_c = app_handle.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    match line {
                        Ok(line) => push_log_line(&logs_c, &app_c, &id_c, &line),
                        Err(_) => break,
                    }
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let id_c = id.to_string();
            let logs_c = Arc::clone(&logs);
            let app_c = app_handle.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => push_log_line(&logs_c, &app_c, &id_c, &line),
                        Err(_) => break,
                    }
                }
            });
        }

        {
            let mut procs = self.processes.lock();
            if let Some(mut old) = procs.insert(id.to_string(), child) {
                // Never drop a live Child — that orphans the process and holds the port.
                kill_child_process(&mut old);
            }
        }
        thread::sleep(Duration::from_millis(700));

        let still_alive = {
            let mut procs = self.processes.lock();
            if let Some(child) = procs.get_mut(id) {
                match child.try_wait() {
                    Ok(None) => true,
                    Ok(Some(status)) => {
                        procs.remove(id);
                        let msg = format!("anytls-client 启动后立即退出 ({status})");
                        {
                            let mut nodes = self.nodes.lock();
                            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                                node.status = NodeStatus::Error;
                                node.pid = None;
                                node.last_error = Some(msg.clone());
                            }
                        }
                        self.append_log(id, &msg);
                        self.emit_nodes();
                        if !silent {
                            return Err(msg);
                        }
                        return Ok(self.list());
                    }
                    Err(e) => {
                        procs.remove(id);
                        let msg = e.to_string();
                        {
                            let mut nodes = self.nodes.lock();
                            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                                node.status = NodeStatus::Error;
                                node.pid = None;
                                node.last_error = Some(msg.clone());
                            }
                        }
                        if !silent {
                            return Err(msg);
                        }
                        return Ok(self.list());
                    }
                }
            } else {
                false
            }
        };

        if still_alive {
            {
                let mut nodes = self.nodes.lock();
                if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                    node.status = NodeStatus::Running;
                    node.last_error = None;
                    node.pid = Some(pid);
                }
            }
            let lan_note = {
                let nodes = self.nodes.lock();
                nodes
                    .iter()
                    .find(|n| n.id == id)
                    .and_then(|n| n.lan_socks.clone())
            };
            if let Some(lan) = lan_note {
                self.append_log(
                    id,
                    format!(
                        "running pid={pid} socks5://127.0.0.1:{local_port} · LAN {lan} ({name_for_log})"
                    ),
                );
            } else {
                self.append_log(
                    id,
                    format!(
                        "running pid={pid} socks5://127.0.0.1:{local_port} ({name_for_log})"
                    ),
                );
            }
            self.emit_nodes();
            self.spawn_health_watch(id, generation);
        }

        // Background watcher for unexpected exits
        let id_watch = id.to_string();
        let processes_w = Arc::clone(&self.processes);
        let nodes_w = Arc::clone(&self.nodes);
        let logs_w = Arc::clone(&self.logs);
        let shutting = Arc::clone(&self.is_shutting_down);
        let app_w = self.app.lock().clone();
        let generations_w = Arc::clone(&self.generations);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            if *shutting.lock() {
                break;
            }
            if generations_w.lock().get(&id_watch).copied().unwrap_or(0).ne(&generation) {
                break;
            }
            let mut procs = processes_w.lock();
            let Some(child) = procs.get_mut(&id_watch) else {
                break;
            };
            match child.try_wait() {
                Ok(None) => continue,
                Ok(Some(status)) => {
                    procs.remove(&id_watch);
                    drop(procs);
                    if *shutting.lock() {
                        break;
                    }
                    {
                        let mut nodes = nodes_w.lock();
                        if let Some(node) = nodes.iter_mut().find(|n| n.id == id_watch) {
                            if node.status == NodeStatus::Stopping {
                                node.status = NodeStatus::Stopped;
                                node.pid = None;
                            } else if matches!(
                                node.status,
                                NodeStatus::Running | NodeStatus::Starting
                            ) {
                                let msg = format!("anytls-client 退出 ({status})");
                                node.status = NodeStatus::Error;
                                node.pid = None;
                                node.last_error = Some(msg.clone());
                                push_log_line(&logs_w, &app_w, &id_watch, &msg);
                            }
                        }
                    }
                    Self::emit_nodes_shared(&nodes_w, &app_w);
                    break;
                }
                Err(_) => {
                    procs.remove(&id_watch);
                    break;
                }
            }
        });

        Ok(self.list())
    }

    pub fn stop(&self, id: &str, silent: bool) -> Result<Vec<ProxyNode>, String> {
        let gate = self.lifecycle_gate(id);
        let _transition = gate.lock();
        self.stop_locked(id, silent)
    }

    // Caller must hold the per-node lifecycle gate. Used by failover to
    // validate node state and stop it atomically.
    fn stop_locked(&self, id: &str, silent: bool) -> Result<Vec<ProxyNode>, String> {
        let exists = self.nodes.lock().iter().any(|n| n.id == id);
        if !exists {
            return if silent {
                Ok(self.list())
            } else {
                Err("节点不存在".into())
            };
        }

        self.bump_generation(id);

        let local_port = {
            let nodes = self.nodes.lock();
            nodes.iter().find(|n| n.id == id).map(|n| n.local_port)
        };

        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.status = NodeStatus::Stopping;
            }
        }
        self.emit_nodes();
        self.append_log(id, "stopping…");

        // Full reclaim: managed child, recorded pid, and any orphan on the port.
        if let Some(port) = local_port {
            self.reclaim_node_runtime(id, port);
        } else if let Some(mut child) = self.processes.lock().remove(id) {
            kill_child_process(&mut child);
        }

        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.status = NodeStatus::Stopped;
                node.pid = None;
            }
        }
        self.append_log(id, "stopped");
        self.emit_nodes();
        Ok(self.list())
    }

    pub fn start_all(&self) -> Result<Vec<ProxyNode>, String> {
        let ids: Vec<String> = self
            .nodes
            .lock()
            .iter()
            .filter(|n| n.enabled && n.status != NodeStatus::Running)
            .map(|n| n.id.clone())
            .collect();
        if ids.is_empty() {
            return Ok(self.list());
        }

        // Surface "starting" immediately so UI updates before long IP select.
        {
            let mut nodes = self.nodes.lock();
            for id in &ids {
                if let Some(node) = nodes.iter_mut().find(|n| n.id == *id) {
                    if node.status != NodeStatus::Running {
                        node.status = NodeStatus::Starting;
                        node.last_error = None;
                        node.last_probe = Some(ProbeStatus {
                            ok: true,
                            ms: None,
                            ip: node.active_ip.clone(),
                            message: if node.prefer_fastest_ip {
                                "排队/并行启动中…".into()
                            } else {
                                "并行启动中…".into()
                            },
                            at: now_iso(),
                        });
                    }
                }
            }
        }
        self.emit_nodes();

        let errors: Mutex<Vec<String>> = Mutex::new(Vec::new());
        thread::scope(|scope| {
            let errors = &errors;
            for id in ids {
                scope.spawn(move || {
                    match self.start(&id, true) {
                        Ok(_) => {
                            let nodes = self.nodes.lock();
                            if let Some(n) = nodes.iter().find(|n| n.id == id) {
                                if n.status == NodeStatus::Error {
                                    errors.lock().push(format!(
                                        "{}: {}",
                                        n.name,
                                        n.last_error
                                            .clone()
                                            .unwrap_or_else(|| "error".into())
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            let name = self
                                .nodes
                                .lock()
                                .iter()
                                .find(|n| n.id == id)
                                .map(|n| n.name.clone())
                                .unwrap_or_else(|| id.clone());
                            errors.lock().push(format!("{name}: {e}"));
                        }
                    }
                });
            }
        });

        let errors = errors.into_inner();
        if !errors.is_empty() {
            return Err(errors.join("\n"));
        }
        Ok(self.list())
    }

    pub fn stop_all(&self) -> Result<Vec<ProxyNode>, String> {
        let ids: Vec<String> = self
            .nodes
            .lock()
            .iter()
            .filter(|n| matches!(n.status, NodeStatus::Running | NodeStatus::Starting))
            .map(|n| n.id.clone())
            .collect();
        if ids.is_empty() {
            return Ok(self.list());
        }

        thread::scope(|scope| {
            for id in ids {
                scope.spawn(move || {
                    let _ = self.stop(&id, true);
                });
            }
        });
        Ok(self.list())
    }

    /// Simple delay test through local SOCKS (Clash-style generate_204 RTT).
    pub fn test(&self, id: &str) -> Result<NodeTestResult, String> {
        let (local_port, status, active_ip) = {
            let nodes = self.nodes.lock();
            let node = nodes.iter().find(|n| n.id == id).ok_or("节点不存在")?;
            (
                node.local_port,
                node.status.clone(),
                node.active_ip.clone(),
            )
        };
        if status != NodeStatus::Running {
            return Err("请先启动节点再测延迟".into());
        }

        self.append_log(id, "simple latency via socks…");
        let socks = format!("127.0.0.1:{local_port}");
        // Lightweight endpoints — measure total time of a tiny HTTP response.
        let urls = [
            "http://www.gstatic.com/generate_204",
            "http://cp.cloudflare.com/",
            "http://captive.apple.com/hotspot-detect.html",
        ];

        let mut last_err = String::from("延迟测试失败");
        let mut best: Option<(u64, String)> = None;

        for url in urls {
            let output = Command::new("curl")
                .args([
                    "-sS",
                    "-o",
                    "/dev/null",
                    "-m",
                    "8",
                    "--connect-timeout",
                    "5",
                    "--socks5-hostname",
                    &socks,
                    "-w",
                    "%{http_code} %{time_total}",
                    url,
                ])
                .output();

            let output = match output {
                Ok(o) => o,
                Err(e) => {
                    last_err = format!("curl 不可用: {e}");
                    break;
                }
            };

            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let mut parts = stdout.split_whitespace();
            let code = parts.next().unwrap_or("");
            let time_s: f64 = parts
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.0);
            let ms = (time_s * 1000.0).round().max(0.0) as u64;
            // Accept common captive-portal / generate_204 status codes.
            let ok_code = matches!(code, "200" | "204" | "301" | "302");

            if output.status.success() && ok_code {
                best = Some((ms.max(1), url.to_string()));
                break;
            }
            last_err = if !stderr.is_empty() {
                format!("延迟测试失败: {stderr}")
            } else if !code.is_empty() && code != "000" {
                format!("延迟测试失败: HTTP {code} ({url})")
            } else {
                format!("延迟测试失败: 无法经 SOCKS 访问 ({url})")
            };
        }

        if let Some((ms, url)) = best {
            let result = NodeTestResult {
                ok: true,
                ms,
                ip: active_ip.clone(),
                message: format!("延迟 {ms}ms"),
                at: now_iso(),
            };
            {
                let mut nodes = self.nodes.lock();
                if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                    node.last_error = None;
                    node.last_test = Some(result.clone());
                }
            }
            self.append_log(id, format!("latency ok: {ms}ms via {url}"));
            self.emit_nodes();
            return Ok(result);
        }

        let result = NodeTestResult {
            ok: false,
            ms: 0,
            ip: active_ip,
            message: last_err.clone(),
            at: now_iso(),
        };
        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                // Don't overwrite last_error for a failed delay probe — keep process health separate.
                node.last_test = Some(result.clone());
            }
        }
        self.append_log(id, format!("latency failed: {last_err}"));
        self.emit_nodes();
        Err(last_err)
    }

    pub fn shutdown(&self) {
        *self.is_shutting_down.lock() = true;
        let _ = self.stop_all();
    }

    pub fn status_summary(&self) -> (usize, usize) {
        let nodes = self.nodes.lock();
        let total = nodes.len();
        let running = nodes
            .iter()
            .filter(|n| n.status == NodeStatus::Running)
            .count();
        (running, total)
    }

    /// Resolve a domain on the physical NIC (DoH), never system DNS / fake-ip.
    /// Used when preferFastestIp is off — still must not pass raw domain to anytls.
    fn resolve_domain_safe(&self, host: &str, port: u16, node_id: &str) -> Result<String, String> {
        self.append_log(
            node_id,
            format!("resolving {host} via physical DoH (preferFastestIp=off) …"),
        );
        // Physical DoH first (223.5.5.5 etc. over en0).
        let mut ips = match net_bind::resolve_a_direct(host) {
            Ok(v) => {
                self.append_log(node_id, format!("DNS/direct: {}", v.join(", ")));
                v
            }
            Err(e) => {
                self.append_log(node_id, format!("DNS/direct: {e}"));
                Vec::new()
            }
        };
        if ips.is_empty() {
            // Last resort: system DNS with fake-ip filter (usually empty under Surge).
            match resolve_system(host, port) {
                Ok(v) => {
                    self.append_log(node_id, format!("DNS/system-fallback: {}", v.join(", ")));
                    ips = v;
                }
                Err(e) => self.append_log(node_id, format!("DNS/system-fallback: {e}")),
            }
        }
        if ips.is_empty() {
            return Err(format!(
                "无法解析 {host}：系统 DNS 可能是 fake-ip，且物理 DoH 失败"
            ));
        }
        // If multiple A records, pick lowest physical tcping (cheap, no iLatency).
        if ips.len() > 1 {
            if let Some((ip, ms, _)) = crate::ip_probe::pick_fastest(&ips, port, TCPING_TIMEOUT) {
                self.append_log(
                    node_id,
                    format!("resolved {host} → {ip} ({ms}ms, local A-record pick)"),
                );
                return Ok(ip);
            }
        }
        let ip = ips.remove(0);
        self.append_log(node_id, format!("resolved {host} → {ip}"));
        Ok(ip)
    }

    /// Choose upstream host as a concrete IP whenever possible.
    /// - Plain IP → as-is
    /// - Domain + preferFastestIp → multi-source + tcping + speed
    /// - Domain without preferFastestIp → physical DoH resolve (never pass domain to anytls)
    fn select_upstream_ip(&self, node: &ProxyNode) -> Result<String, String> {
        let server = node.server.trim();
        if server.is_empty() {
            return Err("服务器地址为空".into());
        }
        if !is_domain_host(server) {
            return Ok(server.to_string());
        }
        if !node.prefer_fastest_ip {
            // CRITICAL: never hand the domain to anytls — it would use system DNS
            // and get Surge/Clash fake-ip (198.18.x), then fail to connect.
            match self.resolve_domain_safe(server, node.port, &node.id) {
                Ok(ip) => {
                    {
                        let mut nodes = self.nodes.lock();
                        if let Some(n) = nodes.iter_mut().find(|n| n.id == node.id) {
                            n.last_probe = Some(ProbeStatus {
                                ok: true,
                                ms: None,
                                ip: Some(ip.clone()),
                                message: format!("DNS 直连解析 {ip}"),
                                at: now_iso(),
                            });
                        }
                    }
                    Ok(ip)
                }
                Err(e) => {
                    self.append_log(
                        &node.id,
                        format!("安全 DNS 失败: {e}；拒绝把域名交给 anytls（防 fake-ip）"),
                    );
                    Err(e)
                }
            }
        } else {
            // Sticky: if we already picked an IP and it is still alive, skip full re-test.
            if let Some(sticky) = node
                .active_ip
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && !is_domain_host(s))
            {
                if let Some(ms) = tcping_with_retries(
                    &sticky,
                    node.port,
                    TCPING_TIMEOUT,
                    TCPING_RETRIES,
                    TCPING_RETRY_GAP,
                ) {
                    self.append_log(
                        &node.id,
                        format!(
                            "reuse sticky IP {sticky} ({ms}ms, alive — skip DNS/speed re-test)"
                        ),
                    );
                    {
                        let mut nodes = self.nodes.lock();
                        if let Some(n) = nodes.iter_mut().find(|n| n.id == node.id) {
                            n.active_ip = Some(sticky.clone());
                            n.last_probe = Some(ProbeStatus {
                                ok: true,
                                ms: Some(ms),
                                ip: Some(sticky.clone()),
                                message: format!("沿用 {sticky} · {ms}ms · 跳过重测"),
                                at: now_iso(),
                            });
                        }
                    }
                    let _ = self.persist();
                    return Ok(sticky);
                }
                self.append_log(
                    &node.id,
                    format!("sticky IP {sticky} dead — full re-select"),
                );
                {
                    let mut nodes = self.nodes.lock();
                    if let Some(n) = nodes.iter_mut().find(|n| n.id == node.id) {
                        n.active_ip = None;
                    }
                }
            }

            self.append_log(
                &node.id,
                format!("selecting fastest IP for {server}:{} …", node.port),
            );

            match self.collect_and_pick(node, &node.id) {
                Ok((ip, ms, mbps, detail)) => {
                    self.append_log(&node.id, detail);
                    let speed_part = mbps
                        .map(|v| format!(" · {v:.2}Mbps"))
                        .unwrap_or_default();
                    self.append_log(
                        &node.id,
                        format!("selected upstream IP {ip} ({ms}ms{speed_part})"),
                    );
                    {
                        let mut nodes = self.nodes.lock();
                        if let Some(n) = nodes.iter_mut().find(|n| n.id == node.id) {
                            n.active_ip = Some(ip.clone());
                            n.last_probe = Some(ProbeStatus {
                                ok: true,
                                ms: Some(ms),
                                ip: Some(ip.clone()),
                                message: format!("优选 {ip} · {ms}ms{speed_part}"),
                                at: now_iso(),
                            });
                        }
                    }
                    let _ = self.persist();
                    Ok(ip)
                }
                Err(err) => {
                    self.append_log(
                        &node.id,
                        format!("IP 优选失败，尝试仅延迟优选: {err}"),
                    );
                    // Second chance: multi-source candidates + latency only (no speed test).
                    let candidates = self.collect_candidate_ips(server, node.port, &node.id);
                    if let Some((ip, ms, ranking)) =
                        crate::ip_probe::pick_fastest(&candidates, node.port, TCPING_TIMEOUT)
                    {
                        let line = ranking
                            .iter()
                            .map(|r| match r.ms {
                                Some(v) => format!("{} {}ms", r.ip, v),
                                None => format!("{} fail", r.ip),
                            })
                            .collect::<Vec<_>>()
                            .join(" · ");
                        self.append_log(&node.id, format!("latency-only ranking: {line}"));
                        self.append_log(
                            &node.id,
                            format!("selected upstream IP {ip} ({ms}ms, latency-only fallback)"),
                        );
                        {
                            let mut nodes = self.nodes.lock();
                            if let Some(n) = nodes.iter_mut().find(|n| n.id == node.id) {
                                n.active_ip = Some(ip.clone());
                                n.last_probe = Some(ProbeStatus {
                                    ok: true,
                                    ms: Some(ms),
                                    ip: Some(ip.clone()),
                                    message: format!("优选 {ip} · {ms}ms · 仅延迟"),
                                    at: now_iso(),
                                });
                            }
                        }
                        let _ = self.persist();
                        Ok(ip)
                    } else {
                        // Never pass bare domain to anytls (Surge fake-ip).
                        self.append_log(
                            &node.id,
                            format!("latency-only 也失败，改为安全 DNS 解析: {err}"),
                        );
                        self.resolve_domain_safe(server, node.port, &node.id)
                    }
                }
            }
        }
    }

    fn collect_candidate_ips(&self, host: &str, port: u16, node_id: &str) -> Vec<String> {
        let mut sets: Vec<Vec<String>> = Vec::new();
        let host_key = host.trim().to_ascii_lowercase();

        // Short-lived multi-source cache (avoid repeat iLatency cost within a few minutes).
        if let Some((at, ips)) = self.dns_cache.lock().get(&host_key).cloned() {
            if at.elapsed() < Duration::from_secs(8 * 60) && ips.len() > 1 {
                self.append_log(
                    node_id,
                    format!(
                        "DNS/cache: {} IPs ({}) age={}s",
                        ips.len(),
                        ips.join(", "),
                        at.elapsed().as_secs()
                    ),
                );
                return ips;
            }
        }

        match resolve_system(host, port) {
            Ok(ips) => {
                self.append_log(
                    node_id,
                    format!("DNS/system: {}", ips.join(", ")),
                );
                sets.push(ips);
            }
            Err(e) => self.append_log(node_id, format!("DNS/system: {e}")),
        }

        let doh = resolve_doh(host);
        if !doh.is_empty() {
            self.append_log(node_id, format!("DNS/DoH: {}", doh.join(", ")));
            sets.push(doh);
        }

        let cookie = self.settings.lock().ilatency_cookie.clone();
        if !cookie.trim().is_empty() {
            self.append_log(node_id, "DNS/iLatency: querying multi-location …");
            let mut ilatency_ips = match ilatency::collect_a_records(host, &cookie) {
                Ok(ips) => {
                    self.append_log(
                        node_id,
                        format!("DNS/iLatency: {} IPs ({})", ips.len(), ips.join(", ")),
                    );
                    ips
                }
                Err(e) => {
                    self.append_log(node_id, format!("DNS/iLatency: {e} · retry once …"));
                    match ilatency::collect_a_records(host, &cookie) {
                        Ok(ips) => {
                            self.append_log(
                                node_id,
                                format!(
                                    "DNS/iLatency(retry): {} IPs ({})",
                                    ips.len(),
                                    ips.join(", ")
                                ),
                            );
                            ips
                        }
                        Err(e2) => {
                            self.append_log(node_id, format!("DNS/iLatency(retry): {e2}"));
                            Vec::new()
                        }
                    }
                }
            };
            if !ilatency_ips.is_empty() {
                sets.push(std::mem::take(&mut ilatency_ips));
            }
        } else {
            self.append_log(
                node_id,
                "DNS/iLatency: skipped (no cookie in settings) — 分运营商域名几乎只会看到本机线路 IP",
            );
        }

        let merged = merge_unique(sets);
        if merged.len() > 1 {
            self.dns_cache
                .lock()
                .insert(host_key, (Instant::now(), merged.clone()));
        }
        merged
    }

    fn collect_and_pick(
        &self,
        node: &ProxyNode,
        node_id: &str,
    ) -> Result<(String, u64, Option<f64>, String), String> {
        let host = node.server.trim();
        let port = node.port;
        let candidates = self.collect_candidate_ips(host, port, node_id);
        if candidates.is_empty() {
            return Err("无可用候选 IP".into());
        }

        let (strategy, top_k, speed_bytes, speed_timeout) = {
            let s = self.settings.lock();
            (
                s.select_strategy,
                // Honor the user's Top-K setting (1..8).
                s.speed_top_k.clamp(1, 8) as usize,
                // The bundled kernel only samples up to 50 MiB per candidate.
                s.speed_test_bytes.clamp(1024 * 1024, 50 * 1024 * 1024),
                Duration::from_secs(s.speed_test_timeout_secs.clamp(10, 60)),
            )
        };

        self.append_log(node_id, net_bind::egress_log_line());
        if net_bind::default_route_is_virtual() {
            self.append_log(
                node_id,
                "hint: 默认路由在 utun/虚拟网卡上（常见于 Surge/Clash 增强模式）。已尝试将 tcping 绑定物理网卡；若结果仍异常，请将节点 IP 加入 Surge 直连/绕过，或临时关闭增强模式后再测。",
            );
        }
        self.append_log(
            node_id,
            format!(
                "candidates ({}): {}",
                candidates.len(),
                candidates.join(", ")
            ),
        );
        if candidates.len() == 1 {
            self.append_log(
                node_id,
                "warning: 仅 1 个候选 IP。分运营商域名需要 iLatency 多地结果；请确认 Cookie 有效且日志含 DNS/iLatency。",
            );
        }
        self.append_log(
            node_id,
            format!(
                "tcping {} candidates on :{port} … (strategy={})",
                candidates.len(),
                strategy.label()
            ),
        );
        let latency_results = probe_all(&candidates, port, TCPING_TIMEOUT);
        if latency_results.iter().all(|r| r.ms.is_none()) {
            return Err("所有候选 IP 的 TCP 连接均失败".into());
        }
        let suspiciously_fast = latency_results
            .iter()
            .filter(|r| matches!(r.ms, Some(ms) if ms < 2))
            .count();
        if suspiciously_fast > 0 {
            self.append_log(
                node_id,
                format!(
                    "warning: {suspiciously_fast} 个 IP 的 tcping <2ms，疑似被透明代理/虚拟网卡劫持，已在排序中降权/忽略"
                ),
            );
        }

        let mut ping_lines = Vec::new();
        for r in &latency_results {
            match r.ms {
                Some(v) => ping_lines.push(format!("{} {}ms", r.ip, v)),
                None => ping_lines.push(format!("{} fail", r.ip)),
            }
        }
        self.append_log(
            node_id,
            format!("tcping ranking: {}", ping_lines.join(" · ")),
        );

        let kernel = self.kernel_path();
        let password = node.password.clone();
        let sni = node.sni.clone();
        if !matches!(strategy, SelectStrategy::Latency) {
            self.append_log(
                node_id,
                format!(
                    "speed-test top {top_k} via temporary anytls ({speed_bytes} bytes, timeout {}s) …",
                    speed_timeout.as_secs()
                ),
            );
        }

        let (best, scored) = rank_and_pick(strategy, &latency_results, top_k, |ip| {
            if !kernel.exists() {
                return Err("找不到 anytls 内核".into());
            }
            measure_proxy_mbps(
                &kernel,
                ip,
                port,
                &password,
                &sni,
                node.skip_cert_verify,
                speed_bytes,
                speed_timeout,
            )
        })?;

        let mut score_lines = Vec::new();
        for s in &scored {
            let mbps = s
                .mbps
                .map(|v| format!("{v:.2}Mbps"))
                .unwrap_or_else(|| "n/a".into());
            score_lines.push(format!(
                "{} {}ms {mbps} score={:.2} ({})",
                s.ip, s.rtt_ms, s.score, s.note
            ));
        }
        // Explain common mis-selection: only one IP got a speed sample.
        let speed_ok = scored.iter().filter(|s| s.mbps.is_some()).count();
        let reason = match strategy {
            SelectStrategy::Throughput if speed_ok == 1 => {
                " · reason=only-one-speed-sample"
            }
            SelectStrategy::Throughput if speed_ok == 0 => " · reason=fallback-latency",
            SelectStrategy::Throughput => " · reason=best-throughput",
            SelectStrategy::Balanced if speed_ok == 0 => " · reason=fallback-latency",
            SelectStrategy::Balanced => " · reason=best-balanced",
            SelectStrategy::Latency => " · reason=best-latency",
        };
        let detail = format!(
            "select[{}] winner={}{} · {}",
            strategy.label(),
            best.ip,
            reason,
            score_lines.join(" · ")
        );
        Ok((best.ip, best.rtt_ms, best.mbps, detail))
    }

    fn spawn_health_watch(&self, id: &str, generation: u64) {
        let id = id.to_string();
        let nodes = Arc::clone(&self.nodes);
        let processes = Arc::clone(&self.processes);
        let logs = Arc::clone(&self.logs);
        let settings = Arc::clone(&self.settings);
        let last_failover_at = Arc::clone(&self.last_failover_at);
        let dns_cache = Arc::clone(&self.dns_cache);
        let starting_ids = Arc::clone(&self.starting_ids);
        let generations = Arc::clone(&self.generations);
        let lifecycle_gates = Arc::clone(&self.lifecycle_gates);
        let shutting = Arc::clone(&self.is_shutting_down);
        let app = self.app.lock().clone();
        // Manager methods need self — re-enter via a lightweight helper object.
        // We only need data_dir for nothing; failover will be inlined carefully.
        let data_dir = self.data_dir.clone();
        let resources_dir = self.resources_dir.clone();
        let store_path = self.store_path.clone();
        let instances_dir = self.instances_dir.clone();

        thread::spawn(move || {
            // Reconstruct a thin handle for failover by sharing arcs.
            let mgr = Manager {
                data_dir,
                resources_dir,
                store_path,
                instances_dir,
                nodes: Arc::clone(&nodes),
                processes: Arc::clone(&processes),
                logs: Arc::clone(&logs),
                settings: Arc::clone(&settings),
                last_failover_at: Arc::clone(&last_failover_at),
                dns_cache: Arc::clone(&dns_cache),
                starting_ids: Arc::clone(&starting_ids),
                is_shutting_down: Arc::clone(&shutting),
                generations: Arc::clone(&generations),
                lifecycle_gates: Arc::clone(&lifecycle_gates),
                app: Mutex::new(app.clone()),
            };

            loop {
                let interval = {
                    let s = settings.lock();
                    Duration::from_secs(s.health_interval_secs.max(3))
                };
                thread::sleep(interval);
                if *shutting.lock() {
                    break;
                }
                if generations.lock().get(&id).copied().unwrap_or(0).ne(&generation) {
                    break;
                }

                let snapshot = {
                    let nodes_g = nodes.lock();
                    nodes_g.iter().find(|n| n.id == id).cloned()
                };
                let Some(node) = snapshot else { break };
                if node.status != NodeStatus::Running {
                    break;
                }
                if !node.prefer_fastest_ip {
                    continue;
                }

                let probe_ip = node
                    .active_ip
                    .clone()
                    .filter(|ip| !ip.is_empty())
                    .unwrap_or_else(|| node.server.clone());

                // Only health-check concrete IPs; domain stays on start-time selection path.
                if is_domain_host(&probe_ip) {
                    continue;
                }

                let ms = tcping_with_retries(
                    &probe_ip,
                    node.port,
                    TCPING_TIMEOUT,
                    TCPING_RETRIES.saturating_sub(1),
                    TCPING_RETRY_GAP,
                );

                {
                    let mut nodes_g = nodes.lock();
                    if let Some(n) = nodes_g.iter_mut().find(|n| n.id == id) {
                        if n.status != NodeStatus::Running {
                            break;
                        }
                        n.last_probe = Some(ProbeStatus {
                            ok: ms.is_some(),
                            ms,
                            ip: Some(probe_ip.clone()),
                            message: match ms {
                                Some(v) => format!("alive {probe_ip} · {v}ms"),
                                None => format!("dead {probe_ip} after {TCPING_RETRIES} tries"),
                            },
                            at: now_iso(),
                        });
                    }
                }
                Manager::emit_nodes_shared(&nodes, &app);

                if ms.is_some() {
                    continue;
                }

                push_log_line(
                    &logs,
                    &app,
                    &id,
                    &format!(
                        "health: {probe_ip}:{} failed {TCPING_RETRIES}× — starting failover",
                        node.port
                    ),
                );

                let cooldown = {
                    let s = settings.lock();
                    Duration::from_secs(s.failover_cooldown_secs.max(15))
                };
                let allowed = {
                    let map = last_failover_at.lock();
                    match map.get(&id) {
                        Some(t) => t.elapsed() >= cooldown,
                        None => true,
                    }
                };
                if !allowed {
                    push_log_line(
                        &logs,
                        &app,
                        &id,
                        "health: failover skipped (cooldown)",
                    );
                    continue;
                }

                if let Err(e) = mgr.failover_upstream(&id) {
                    push_log_line(&logs, &app, &id, &format!("health: failover failed: {e}"));
                }
            }
        });
    }

    /// Re-resolve candidates, pick fastest reachable IP, restart process if needed.
    pub fn failover_upstream(&self, id: &str) -> Result<(), String> {
        // Record the generation before slow network probes, so results from
        // an old node session cannot resurrect a user-stopped connection.
        let gate = self.lifecycle_gate(id);
        let (node, observed_generation) = {
            let _transition = gate.lock();
            let nodes = self.nodes.lock();
            let node = nodes
                .iter()
                .find(|n| n.id == id)
                .cloned()
                .ok_or_else(|| "节点不存在".to_string())?;
            let generation = self.generations.lock().get(id).copied().unwrap_or(0);
            (node, generation)
        };
        if node.status != NodeStatus::Running {
            return Err("节点未运行".into());
        }
        if !node.prefer_fastest_ip {
            return Err("未启用优选最快 IP".into());
        }
        if !is_domain_host(&node.server) {
            return Err("服务器已是 IP，无法多地重解析".into());
        }

        self.last_failover_at
            .lock()
            .insert(id.to_string(), Instant::now());

        let (best_ip, ms, mbps, detail) = self.collect_and_pick(&node, id)?;
        self.append_log(id, detail);
        let speed_part = mbps
            .map(|v| format!(" · {v:.2}Mbps"))
            .unwrap_or_default();

        let current = node
            .active_ip
            .clone()
            .unwrap_or_else(|| node.server.clone());
        if best_ip == current {
            // Still the same IP — verify once more; if dead, nothing better available.
            if tcping_with_retries(
                &best_ip,
                node.port,
                TCPING_TIMEOUT,
                TCPING_RETRIES.saturating_sub(1),
                TCPING_RETRY_GAP,
            )
            .is_none()
            {
                return Err(format!(
                    "最优 IP 仍是 {best_ip} 且不可达，保持当前进程"
                ));
            }
            self.append_log(
                id,
                format!("failover: best IP unchanged ({best_ip}, {ms}ms{speed_part})"),
            );
            return Ok(());
        }

        // Hold the gate across validation and Stop. Manual user actions that
        // happened during selection invalidate this failover generation.
        let _transition = gate.lock();
        let now_generation = self.generations.lock().get(id).copied().unwrap_or(0);
        let same_running_node = self.nodes.lock().iter().any(|n| {
            n.id == id && n.status == NodeStatus::Running
                && n.server == node.server && n.port == node.port
        });
        if now_generation != observed_generation || !same_running_node {
            return Err("节点状态已变化，取消过期的自动切换".into());
        }
        self.append_log(
            id,
            format!("failover: {current} → {best_ip} ({ms}ms{speed_part}), restarting …"),
        );
        {
            let mut nodes = self.nodes.lock();
            if let Some(n) = nodes.iter_mut().find(|n| n.id == id) {
                n.active_ip = Some(best_ip.clone());
                n.last_probe = Some(ProbeStatus {
                    ok: true,
                    ms: Some(ms),
                    ip: Some(best_ip.clone()),
                    message: format!("failover → {best_ip} · {ms}ms{speed_part}"),
                    at: now_iso(),
                });
            }
        }
        self.persist()?;
        self.stop_locked(id, true)?;
        let stop_generation = self.generations.lock().get(id).copied().unwrap_or(0);
        drop(_transition);
        // Start only if no user Stop or edit intervened after failover's Stop.
        self.start_with_upstream(id, &best_ip, true, Some(stop_generation))?;
        Ok(())
    }

    /// Manual "re-select IP" (user-triggered).
    /// - Running node: clear sticky, re-run full DNS→tcping→speed, restart if changed.
    /// - Stopped node: clear sticky so next start does a fresh full select.
    pub fn reselect_upstream(&self, id: &str) -> Result<(), String> {
        let node = {
            let nodes = self.nodes.lock();
            nodes
                .iter()
                .find(|n| n.id == id)
                .cloned()
                .ok_or_else(|| "节点不存在".to_string())?
        };
        if !node.prefer_fastest_ip {
            return Err("未启用优选最快 IP".into());
        }
        if !is_domain_host(&node.server) {
            return Err("服务器已是 IP，无需重选".into());
        }

        if node.status == NodeStatus::Running {
            // Full pipeline already implemented in failover_upstream (re-resolve +
            // tcping + speed + restart-if-changed). Single owner for re-selection.
            return self.failover_upstream(id);
        }

        // Not running: drop sticky so the next start re-selects from scratch.
        {
            let mut nodes = self.nodes.lock();
            if let Some(n) = nodes.iter_mut().find(|n| n.id == id) {
                n.active_ip = None;
                n.last_probe = None;
            }
        }
        self.persist()?;
        self.emit_nodes();
        self.append_log(
            id,
            "重测：已清除粘性 IP，下次启动将重新优选（DNS → tcping → 测速）",
        );
        Ok(())
    }

    /// Start using a fixed upstream host/IP (skips re-selection).
    fn start_with_upstream(
        &self,
        id: &str,
        upstream_host: &str,
        silent: bool,
        expected_generation: Option<u64>,
    ) -> Result<Vec<ProxyNode>, String> {
        let _guard = match self.begin_start_guard(id) {
            Some(g) => g,
            None => {
                if silent {
                    return Ok(self.list());
                }
                return Err("节点正在启动中".into());
            }
        };

        let transition_gate = self.lifecycle_gate(id);
        let _transition = transition_gate.lock();
        if let Some(expected) = expected_generation {
            if self.generations.lock().get(id).copied().unwrap_or(0) != expected {
                return self.cancelled_start(silent);
            }
        }
        let status = {
            let nodes = self.nodes.lock();
            nodes.iter().find(|n| n.id == id).ok_or("节点不存在")?.status.clone()
        };
        if status == NodeStatus::Running && self.processes.lock().contains_key(id) {
            return Ok(self.list());
        }
        let generation = self.bump_generation(id);

        let kernel = self.kernel_path();
        if !kernel.exists() {
            return self.abort_start(id, ["找不到 anytls 内核: ", kernel.display().to_string().as_str()].concat(), silent);
        }

        let (local_port, name_for_log) = {
            let nodes = self.nodes.lock();
            let node = nodes.iter().find(|n| n.id == id).ok_or("节点不存在")?;
            (node.local_port, node.name.clone())
        };
        self.reclaim_node_runtime(id, local_port);
        if let Err(e) = self.assert_port_available(local_port, id) {
            kill_owned_listeners_on_port(local_port, &kernel);
            thread::sleep(Duration::from_millis(100));
            return self.abort_start(id, e, silent);
        }

        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.status = NodeStatus::Starting;
                node.pid = None;
                node.last_error = None;
                node.active_ip = Some(upstream_host.to_string());
            }
        }
        self.emit_nodes();

        let node_snapshot = {
            let nodes = self.nodes.lock();
            nodes
                .iter()
                .find(|n| n.id == id)
                .cloned()
                .ok_or("节点不存在")?
        };

        let dir = match self.instance_dir(&node_snapshot.id) {
            Ok(dir) => dir,
            Err(e) => return self.abort_start(id, e, silent),
        };
        let listen = node_snapshot.listen_addr();
        let server = format!("{}:{}", upstream_host, node_snapshot.port);
        let sni = if node_snapshot.sni.is_empty() {
            "www.icloud.com".to_string()
        } else {
            node_snapshot.sni.clone()
        };

        self.append_log(
            id,
            format!("starting anytls-client on socks {listen}"),
        );
        self.append_log(
            id,
            format!(
                "upstream: {} → {server} sni={sni}",
                node_snapshot.server
            ),
        );

        let bind_args = net_bind::anytls_bind_args();
        if !bind_args.is_empty() {
            self.append_log(id, format!("anytls physical bind: {}", bind_args.join(" ")));
        }
        let mut args: Vec<String> = vec![
            "-l".into(),
            listen.clone(),
            "-s".into(),
            server.clone(),
            "-sni".into(),
            sni.clone(),
        ];
        if node_snapshot.skip_cert_verify {
            args.push("-insecure".into());
        }
        args.extend(bind_args);
        let mut child = match Command::new(&kernel)
            .args(&args)
            .env("SOCKSNODE_PASSWORD", &node_snapshot.password)
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => return self.abort_start(id, e.to_string(), silent),
        };

        let pid = child.id();
        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.pid = Some(pid);
            }
        }

        let logs = Arc::clone(&self.logs);
        let app_handle = self.app.lock().clone();
        if let Some(stdout) = child.stdout.take() {
            let id_c = id.to_string();
            let logs_c = Arc::clone(&logs);
            let app_c = app_handle.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines() {
                    match line {
                        Ok(line) => push_log_line(&logs_c, &app_c, &id_c, &line),
                        Err(_) => break,
                    }
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let id_c = id.to_string();
            let logs_c = Arc::clone(&logs);
            let app_c = app_handle.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines() {
                    match line {
                        Ok(line) => push_log_line(&logs_c, &app_c, &id_c, &line),
                        Err(_) => break,
                    }
                }
            });
        }

        {
            let mut procs = self.processes.lock();
            if let Some(mut old) = procs.insert(id.to_string(), child) {
                kill_child_process(&mut old);
            }
        }
        thread::sleep(Duration::from_millis(700));

        let still_alive = {
            let mut procs = self.processes.lock();
            if let Some(child) = procs.get_mut(id) {
                match child.try_wait() {
                    Ok(None) => true,
                    Ok(Some(status)) => {
                        procs.remove(id);
                        let msg = format!("anytls-client 启动后立即退出 ({status})");
                        {
                            let mut nodes = self.nodes.lock();
                            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                                node.status = NodeStatus::Error;
                                node.pid = None;
                                node.last_error = Some(msg.clone());
                            }
                        }
                        self.append_log(id, &msg);
                        self.emit_nodes();
                        if !silent {
                            return Err(msg);
                        }
                        return Ok(self.list());
                    }
                    Err(e) => {
                        procs.remove(id);
                        let msg = e.to_string();
                        {
                            let mut nodes = self.nodes.lock();
                            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                                node.status = NodeStatus::Error;
                                node.pid = None;
                                node.last_error = Some(msg.clone());
                            }
                        }
                        if !silent {
                            return Err(msg);
                        }
                        return Ok(self.list());
                    }
                }
            } else {
                false
            }
        };

        if still_alive {
            {
                let mut nodes = self.nodes.lock();
                if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                    node.status = NodeStatus::Running;
                    node.last_error = None;
                    node.pid = Some(pid);
                    node.active_ip = Some(upstream_host.to_string());
                }
            }
            let lan_note = {
                let nodes = self.nodes.lock();
                nodes
                    .iter()
                    .find(|n| n.id == id)
                    .and_then(|n| n.lan_socks.clone())
            };
            if let Some(lan) = lan_note {
                self.append_log(
                    id,
                    format!(
                        "running pid={pid} socks5://127.0.0.1:{local_port} · LAN {lan} ({name_for_log})"
                    ),
                );
            } else {
                self.append_log(
                    id,
                    format!(
                        "running pid={pid} socks5://127.0.0.1:{local_port} ({name_for_log})"
                    ),
                );
            }
            self.emit_nodes();
            self.spawn_health_watch(id, generation);
        }

        // exit watcher (same as start)
        let id_watch = id.to_string();
        let processes_w = Arc::clone(&self.processes);
        let nodes_w = Arc::clone(&self.nodes);
        let logs_w = Arc::clone(&self.logs);
        let shutting = Arc::clone(&self.is_shutting_down);
        let app_w = self.app.lock().clone();
        let generations_w = Arc::clone(&self.generations);
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            if *shutting.lock() {
                break;
            }
            if generations_w.lock().get(&id_watch).copied().unwrap_or(0).ne(&generation) {
                break;
            }
            let mut procs = processes_w.lock();
            let Some(child) = procs.get_mut(&id_watch) else {
                break;
            };
            match child.try_wait() {
                Ok(None) => continue,
                Ok(Some(status)) => {
                    procs.remove(&id_watch);
                    drop(procs);
                    if *shutting.lock() {
                        break;
                    }
                    {
                        let mut nodes = nodes_w.lock();
                        if let Some(node) = nodes.iter_mut().find(|n| n.id == id_watch) {
                            if node.status == NodeStatus::Stopping {
                                node.status = NodeStatus::Stopped;
                                node.pid = None;
                            } else if matches!(
                                node.status,
                                NodeStatus::Running | NodeStatus::Starting
                            ) {
                                let msg = format!("anytls-client 退出 ({status})");
                                node.status = NodeStatus::Error;
                                node.pid = None;
                                node.last_error = Some(msg.clone());
                                push_log_line(&logs_w, &app_w, &id_watch, &msg);
                            }
                        }
                    }
                    Self::emit_nodes_shared(&nodes_w, &app_w);
                    break;
                }
                Err(_) => {
                    procs.remove(&id_watch);
                    break;
                }
            }
        });

        Ok(self.list())
    }
}

fn push_log_line(logs: &SharedLogs, app: &Option<AppHandle>, id: &str, line: &str) {
    let entry = LogEntry {
        ts: now_iso(),
        line: line.trim_end().to_string(),
    };
    eprintln!("[socks-node:{id}] {}", entry.line);
    {
        let mut map = logs.lock();
        let list = map.entry(id.to_string()).or_default();
        list.push(entry.clone());
        while list.len() > 500 {
            list.remove(0);
        }
    }
    if let Some(app) = app {
        let _ = app.emit(
            "nodes:log",
            LogEvent {
                id: id.to_string(),
                ts: entry.ts,
                line: entry.line,
            },
        );
    }
}

pub fn resolve_paths(app: &AppHandle) -> (PathBuf, PathBuf) {
    let data_dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from(".").join("socks-node-data"))
        .join("socks-node");
    let _ = std::fs::create_dir_all(&data_dir);

    let resources_dir = app
        .path()
        .resource_dir()
        .unwrap_or_else(|_| PathBuf::from("src-tauri/resources"));

    let dev_resources = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
    let resources_dir = if dev_resources.join("anytls-client").exists() {
        dev_resources
    } else if resources_dir.join("anytls-client").exists() {
        resources_dir
    } else if resources_dir
        .join("resources")
        .join("anytls-client")
        .exists()
    {
        resources_dir.join("resources")
    } else {
        resources_dir
    };

    (data_dir, resources_dir)
}

pub fn open_path(path: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manager(enabled: bool) -> (Manager, PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("socksnode-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let manager = Manager::new(dir.clone(), dir.join("resources"));
        let nodes = manager
            .create(NodeInput {
                name: "Test node".into(),
                proxy_type: "anytls".into(),
                server: "127.0.0.1".into(),
                port: 443,
                password: "test-only-password".into(),
                udp: true,
                sni: "example.com".into(),
                skip_cert_verify: false,
                local_port: 58983,
                enabled,
                prefer_fastest_ip: false,
                lan_access: false,
            })
            .unwrap();
        let id = nodes[0].id.clone();
        (manager, dir, id)
    }

    #[test]
    fn stopping_an_absent_node_returns_without_deadlocking() {
        let (manager, dir, _) = test_manager(false);
        assert!(manager.stop("missing-node-id", true).unwrap().len() == 1);
        assert!(manager.stop("missing-node-id", false).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn user_stop_cancels_a_stale_failover_restart() {
        let (manager, dir, id) = test_manager(true);
        manager.nodes.lock()[0].status = NodeStatus::Running;
        let old_generation = manager.bump_generation(&id);
        manager.stop(&id, true).unwrap();
        assert!(!manager.generation_is_current(&id, old_generation));

        // An obsolete failover must not spawn a new process after user Stop.
        let state = manager
            .start_with_upstream(&id, "127.0.0.1", true, Some(old_generation))
            .unwrap();
        assert_eq!(state[0].status, NodeStatus::Stopped);
        assert!(manager.processes.lock().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn disabled_nodes_are_not_started_by_start_all() {
        let (manager, dir, id) = test_manager(false);
        manager.start_all().unwrap();
        assert_eq!(
            manager.list().into_iter().find(|node| node.id == id).unwrap().status,
            NodeStatus::Stopped
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_edit_must_not_stop_running_node() {
        let (manager, dir, id) = test_manager(true);
        manager.nodes.lock()[0].status = NodeStatus::Running;
        let mut patch = manager.list()[0].clone();
        patch.server.clear();
        let error = manager
            .update(
                &id,
                NodeInput {
                    name: patch.name,
                    proxy_type: patch.proxy_type,
                    server: patch.server,
                    port: patch.port,
                    password: patch.password,
                    udp: patch.udp,
                    sni: patch.sni,
                    skip_cert_verify: patch.skip_cert_verify,
                    local_port: patch.local_port,
                    enabled: patch.enabled,
                    prefer_fastest_ip: patch.prefer_fastest_ip,
                    lan_access: patch.lan_access,
                },
            )
            .unwrap_err();
        assert!(error.contains("服务器地址"));
        assert_eq!(manager.nodes.lock()[0].status, NodeStatus::Running);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stopping_invalidates_a_pending_start() {
        let (manager, dir, id) = test_manager(true);
        let generation = manager.bump_generation(&id);
        manager.nodes.lock()[0].status = NodeStatus::Starting;

        manager.stop(&id, true).unwrap();

        assert!(!manager.generation_is_current(&id, generation));
        assert_eq!(manager.nodes.lock()[0].status, NodeStatus::Stopped);
        assert!(manager.cancelled_start(false).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn duplicate_start_does_not_lock_nodes_twice() {
        let (manager, dir, id) = test_manager(true);
        let child = Command::new("sleep")
            .arg("20")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        manager.processes.lock().insert(id.clone(), child);
        manager.nodes.lock()[0].status = NodeStatus::Running;

        assert_eq!(manager.start(&id, false).unwrap()[0].status, NodeStatus::Running);
        assert_eq!(
            manager.start_with_upstream(&id, "127.0.0.1", false, None).unwrap()[0].status,
            NodeStatus::Running
        );
        assert_eq!(manager.processes.lock().len(), 1);
        let mut child = manager.processes.lock().remove(&id).unwrap();
        kill_child_process(&mut child);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
