use crate::models::{
    default_nodes, now_iso, store_from_json, store_to_json, AppInfo, LogEntry, LogEvent, NodeInput,
    NodeStatus, NodeTestResult, ProxyNode,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager as TauriManager};

type SharedLogs = Arc<Mutex<HashMap<String, Vec<LogEntry>>>>;
type SharedProcs = Arc<Mutex<HashMap<String, Child>>>;
type SharedNodes = Arc<Mutex<Vec<ProxyNode>>>;

pub struct Manager {
    data_dir: PathBuf,
    resources_dir: PathBuf,
    store_path: PathBuf,
    instances_dir: PathBuf,
    nodes: SharedNodes,
    processes: SharedProcs,
    logs: SharedLogs,
    is_shutting_down: Arc<Mutex<bool>>,
    app: Mutex<Option<AppHandle>>,
}

impl Manager {
    pub fn new(data_dir: PathBuf, resources_dir: PathBuf) -> Self {
        let store_path = data_dir.join("nodes.json");
        let instances_dir = data_dir.join("instances");
        Self {
            data_dir,
            resources_dir,
            store_path,
            instances_dir,
            nodes: Arc::new(Mutex::new(Vec::new())),
            processes: Arc::new(Mutex::new(HashMap::new())),
            logs: Arc::new(Mutex::new(HashMap::new())),
            is_shutting_down: Arc::new(Mutex::new(false)),
            app: Mutex::new(None),
        }
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

    fn persist_nodes(&self, nodes: &[ProxyNode]) -> Result<(), String> {
        let json = store_to_json(nodes).map_err(|e| e.to_string())?;
        std::fs::write(&self.store_path, json).map_err(|e| e.to_string())
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
            mihomo_path: self.kernel_path().display().to_string(),
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
        {
            let mut logs = self.logs.lock();
            let list = logs.entry(id.to_string()).or_default();
            list.push(entry.clone());
            while list.len() > 400 {
                list.remove(0);
            }
        }
        if let Some(app) = self.app.lock().as_ref() {
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
        })
    }

    pub fn create(&self, input: NodeInput) -> Result<Vec<ProxyNode>, String> {
        let data = self.normalize_input(&input, None)?;
        let node = ProxyNode {
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
            status: NodeStatus::Stopped,
            pid: None,
            last_error: None,
            last_test: None,
            updated_at: Some(now_iso()),
            socks: format!("socks5://127.0.0.1:{}", data.local_port),
            running: false,
        };
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
        let was_running = {
            let nodes = self.nodes.lock();
            let node = nodes.iter().find(|n| n.id == id).ok_or("节点不存在")?;
            node.status == NodeStatus::Running
        };
        if was_running {
            let _ = self.stop(id, true);
        }

        let data = self.normalize_input(&patch, Some(id))?;
        {
            let mut nodes = self.nodes.lock();
            let node = nodes.iter_mut().find(|n| n.id == id).ok_or("节点不存在")?;
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
                .map(|n| n.status == NodeStatus::Running)
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
        self.persist()?;
        let dir = self.instances_dir.join(id);
        let _ = std::fs::remove_dir_all(dir);
        self.emit_nodes();
        Ok(self.list())
    }

    fn assert_port_available(&self, port: u16, exclude_id: &str) -> Result<(), String> {
        let nodes = self.nodes.lock();
        if let Some(conflict) = nodes.iter().find(|n| {
            n.id != exclude_id && n.local_port == port && n.status == NodeStatus::Running
        }) {
            return Err(format!(
                "本地端口 {} 已被节点「{}」占用",
                port, conflict.name
            ));
        }
        Ok(())
    }

    pub fn start(&self, id: &str, silent: bool) -> Result<Vec<ProxyNode>, String> {
        {
            let nodes = self.nodes.lock();
            let node = nodes.iter().find(|n| n.id == id).ok_or("节点不存在")?;
            if node.status == NodeStatus::Running {
                return Ok(self.list());
            }
        }

        let kernel = self.kernel_path();
        if !kernel.exists() {
            return Err(format!("找不到 anytls 内核: {}", kernel.display()));
        }

        let (local_port, name_for_log) = {
            let nodes = self.nodes.lock();
            let node = nodes.iter().find(|n| n.id == id).ok_or("节点不存在")?;
            (node.local_port, node.name.clone())
        };
        self.assert_port_available(local_port, id)?;

        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.status = NodeStatus::Starting;
                node.last_error = None;
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

        let dir = self.instance_dir(&node_snapshot.id)?;
        let listen = format!("127.0.0.1:{}", node_snapshot.local_port);
        let server = format!("{}:{}", node_snapshot.server, node_snapshot.port);
        let sni = if node_snapshot.sni.is_empty() {
            "www.icloud.com".to_string()
        } else {
            node_snapshot.sni.clone()
        };

        self.append_log(
            id,
            format!("starting anytls-client on socks {listen}"),
        );
        self.append_log(id, format!("upstream: {server} sni={sni}"));

        let mut child = Command::new(&kernel)
            .args([
                "-l",
                &listen,
                "-s",
                &server,
                "-p",
                &node_snapshot.password,
                "-sni",
                &sni,
            ])
            .current_dir(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("启动 anytls-client 失败: {e}"))?;

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
                for line in reader.lines().flatten() {
                    push_log_line(&logs_c, &app_c, &id_c, &line);
                }
            });
        }
        if let Some(stderr) = child.stderr.take() {
            let id_c = id.to_string();
            let logs_c = Arc::clone(&logs);
            let app_c = app_handle.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().flatten() {
                    push_log_line(&logs_c, &app_c, &id_c, &line);
                }
            });
        }

        self.processes.lock().insert(id.to_string(), child);
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
            self.append_log(
                id,
                format!(
                    "running pid={pid} socks5://127.0.0.1:{} ({name_for_log})",
                    local_port
                ),
            );
            self.emit_nodes();
        }

        // Background watcher for unexpected exits
        let id_watch = id.to_string();
        let processes_w = Arc::clone(&self.processes);
        let nodes_w = Arc::clone(&self.nodes);
        let logs_w = Arc::clone(&self.logs);
        let shutting = Arc::clone(&self.is_shutting_down);
        let app_w = self.app.lock().clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(500));
            if *shutting.lock() {
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
        {
            let nodes = self.nodes.lock();
            if nodes.iter().find(|n| n.id == id).is_none() {
                if silent {
                    return Ok(self.list());
                }
                return Err("节点不存在".into());
            }
        }

        let mut child_opt = self.processes.lock().remove(id);
        if child_opt.is_none() {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.status = NodeStatus::Stopped;
                node.pid = None;
            }
            drop(nodes);
            self.emit_nodes();
            return Ok(self.list());
        }

        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.status = NodeStatus::Stopping;
            }
        }
        self.emit_nodes();
        self.append_log(id, "stopping…");

        if let Some(mut child) = child_opt.take() {
            let _ = child.kill();
            let _ = child.wait();
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
            .filter(|n| n.status != NodeStatus::Running)
            .map(|n| n.id.clone())
            .collect();
        let mut errors = Vec::new();
        for id in ids {
            match self.start(&id, true) {
                Ok(_) => {
                    let nodes = self.nodes.lock();
                    if let Some(n) = nodes.iter().find(|n| n.id == id) {
                        if n.status == NodeStatus::Error {
                            errors.push(format!(
                                "{}: {}",
                                n.name,
                                n.last_error.clone().unwrap_or_else(|| "error".into())
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
                        .unwrap_or(id);
                    errors.push(format!("{name}: {e}"));
                }
            }
        }
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
        for id in ids {
            let _ = self.stop(&id, true);
        }
        Ok(self.list())
    }

    pub fn test(&self, id: &str) -> Result<NodeTestResult, String> {
        let (local_port, status) = {
            let nodes = self.nodes.lock();
            let node = nodes.iter().find(|n| n.id == id).ok_or("节点不存在")?;
            (node.local_port, node.status.clone())
        };
        if status != NodeStatus::Running {
            return Err("请先启动节点再测试连通性".into());
        }

        self.append_log(id, "testing connectivity via socks…");
        let started = std::time::Instant::now();
        let socks = format!("127.0.0.1:{local_port}");

        let output = Command::new("curl")
            .args([
                "-sS",
                "-m",
                "15",
                "--socks5-hostname",
                &socks,
                "https://api.ip.sb/ip",
            ])
            .output()
            .map_err(|e| e.to_string())?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let ms = started.elapsed().as_millis() as u64;

        if !output.status.success() || stdout.is_empty() {
            let message = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                "连通性测试失败：无法通过该 SOCKS 访问外网".into()
            };
            let result = NodeTestResult {
                ok: false,
                ms,
                ip: None,
                message: message.clone(),
                at: now_iso(),
            };
            {
                let mut nodes = self.nodes.lock();
                if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                    node.last_error = Some(message.clone());
                    node.last_test = Some(result.clone());
                }
            }
            self.append_log(id, format!("test failed: {message}"));
            self.emit_nodes();
            return Err(message);
        }

        let ip = stdout
            .split_whitespace()
            .next()
            .unwrap_or(&stdout)
            .to_string();
        let result = NodeTestResult {
            ok: true,
            ms,
            ip: Some(ip.clone()),
            message: format!("出口 IP {ip} · {ms}ms"),
            at: now_iso(),
        };
        {
            let mut nodes = self.nodes.lock();
            if let Some(node) = nodes.iter_mut().find(|n| n.id == id) {
                node.last_error = None;
                node.last_test = Some(result.clone());
            }
        }
        self.append_log(id, format!("test ok: egress {ip} in {ms}ms"));
        self.emit_nodes();
        Ok(result)
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
}

fn push_log_line(logs: &SharedLogs, app: &Option<AppHandle>, id: &str, line: &str) {
    let entry = LogEntry {
        ts: now_iso(),
        line: line.trim_end().to_string(),
    };
    {
        let mut map = logs.lock();
        let list = map.entry(id.to_string()).or_default();
        list.push(entry.clone());
        while list.len() > 400 {
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
