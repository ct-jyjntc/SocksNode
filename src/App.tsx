import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight, Check, Copy, FolderOpen, GitBranch, Moon, Plus, RefreshCw, Search, Settings2, Sun, X, Network, Activity, RotateCcw } from "lucide-react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./lib/api";
import type { AppInfo, AppSettingsPatch, AppSettingsView, LogEntry, NodeInput, ProxyNode, SelectStrategy } from "./lib/types";

type Page = "nodes" | "upstreams" | "gateway" | "routing" | "latency" | "logs" | "settings";
type SettingsTab = "general" | "select" | "network" | "ilatency" | "about";
type Tone = "ok" | "err" | "warn";
type Theme = "system" | "light" | "dark";
type LogFilter = "all" | "ok" | "warn";

const NAV: [Page, string][] = [["nodes", "节点"], ["upstreams", "上游"], ["gateway", "网关"], ["routing", "优选"], ["latency", "延迟"], ["logs", "日志"]];
const STRATEGIES: [SelectStrategy, string][] = [["throughput", "高带宽"], ["balanced", "均衡"], ["latency", "低延迟"]];
const SNI_PRESETS = ["www.icloud.com", "gateway.icloud.com", "www.microsoft.com", "www.apple.com", "dl.google.com"];
const BASE_FORM: NodeInput = { name: "", type: "anytls", server: "", port: 443, password: "", sni: "www.icloud.com", localPort: 5001, enabled: true, udp: true, preferFastestIp: true, skipCertVerify: true, lanAccess: false };
const cn = (...values: (string | false | null | undefined)[]) => values.filter(Boolean).join(" ");
const flag = (name: string) => /香港|hk\b/i.test(name) ? "🇭🇰" : /日本|东京|大阪|jp\b/i.test(name) ? "🇯🇵" : /新加坡|sg\b/i.test(name) ? "🇸🇬" : /美国|洛杉矶|us\b/i.test(name) ? "🇺🇸" : /德国|法兰克福|de\b/i.test(name) ? "🇩🇪" : /台湾|tw\b/i.test(name) ? "🇹🇼" : /韩国|首尔|kr\b/i.test(name) ? "🇰🇷" : "🌐";
const dotClass = (status: ProxyNode["status"]) => status === "running" ? "on" : status === "error" ? "err" : status === "starting" || status === "stopping" ? "busy" : "";
const latClass = (value: number | null | undefined) => value == null ? "" : value < 100 ? "lat-g" : value < 250 ? "lat-y" : "lat-r";
const runtimeLatency = (node: ProxyNode) => node.lastTest?.ok ? node.lastTest.ms : node.lastProbe?.ms ?? null;
const fmtTime = (value: string) => { const date = new Date(value); return Number.isNaN(date.getTime()) ? value : date.toLocaleTimeString("zh-CN", { hour12: false }); };
const errMessage = (error: unknown) => error instanceof Error ? error.message : String(error || "操作失败");
function nextPort(nodes: ProxyNode[]) { const used = new Set(nodes.map(n => n.localPort)); let p = 5001; while (used.has(p) && p < 65535) p++; return p; }
function toInput(node: ProxyNode): NodeInput { return { name: node.name, type: "anytls", server: node.server, port: node.port, password: node.password, sni: node.sni, localPort: node.localPort, enabled: node.enabled, udp: node.udp, preferFastestIp: node.preferFastestIp, skipCertVerify: node.skipCertVerify, lanAccess: node.lanAccess }; }
function anytlsLink(node: ProxyNode) {
  const url = new URL("anytls://placeholder");
  url.hostname = node.server.includes(":") && !node.server.startsWith("[") ? "[" + node.server + "]" : node.server;
  url.port = String(node.port);
  url.username = encodeURIComponent(node.password);
  url.searchParams.set("sni", node.sni);
  url.hash = encodeURIComponent(node.name);
  return url.toString();
}
function readAnytlsLink(text: string): Partial<NodeInput> {
  const url = new URL(text.trim());
  if (url.protocol !== "anytls:" || !url.hostname || !url.port || !url.username) throw new Error("请输入包含密码、服务器和端口的 anytls:// 链接");
  const port = Number(url.port);
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error("远程端口无效");
  return { name: decodeURIComponent(url.hash.slice(1)) || url.hostname, server: url.hostname.replace(/^\[|\]$/g, ""), port, password: decodeURIComponent(url.username), sni: url.searchParams.get("sni") || "www.icloud.com" };
}
function IconBtn({ title, onClick, children, disabled }: { title: string; onClick: () => void; children: ReactNode; disabled?: boolean }) {
  return <button className="icon" title={title} aria-label={title} disabled={disabled} onClick={onClick}>{children}</button>;
}
function TextBtn({ children, onClick, kind, disabled, title }: { children: ReactNode; onClick: () => void; kind?: "primary" | "action" | "danger"; disabled?: boolean; title?: string }) {
  return <button className={cn("text", kind)} title={title} disabled={disabled} onClick={onClick}>{children}</button>;
}
function Toggle({ checked, onClick, busy, label, disabled }: { checked: boolean; onClick: () => void; busy?: boolean; label: string; disabled?: boolean }) {
  return <button role="switch" aria-checked={checked} aria-label={label} title={label} className={cn("tg", checked && "on", busy && "busy")} disabled={disabled || busy} onClick={onClick} />;
}
function RowHead({ label, right }: { label: string; right?: ReactNode }) { return <div className="row-head"><span className="label">{label}</span><span className="grow" />{right}</div>; }
function Empty({ title, children, action }: { title: string; children?: ReactNode; action?: ReactNode }) { return <div className="list"><div className="empty-state"><b>{title}</b>{children}{action && <div style={{ marginTop: 12 }}>{action}</div>}</div></div>; }
function PrefRow({ title, description, control }: { title: string; description: ReactNode; control: ReactNode }) { return <div className="row pref"><div className="who"><div className="name">{title}</div><div className="sub wrap">{description}</div></div>{control}</div>; }
function Segment<T extends string>({ value, options, onChange, disabled }: { value: T; options: [T, string][]; onChange: (value: T) => void; disabled?: boolean }) {
  return <div className="segs">{options.map(([key, text]) => <button key={key} className={cn(value === key && "on")} disabled={disabled} onClick={() => onChange(key)}>{text}</button>)}</div>;
}
function Field({ label, children }: { label: string; children: ReactNode }) { return <><label>{label}</label>{children}</>; }
function NumericSetting({ value, unit, min, max, disabled, onSave }: { value: number; unit: string; min: number; max: number; disabled: boolean; onSave: (v: number) => void }) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);
  const commit = () => {
    const n = Number(draft);
    if (!Number.isFinite(n) || !Number.isInteger(n) || n < min || n > max) { setDraft(String(value)); return; }
    if (n !== value) onSave(n);
  };
  return <span style={{ display: "inline-flex", gap: 6, alignItems: "center" }}><input className="numin" type="number" min={min} max={max} value={draft} disabled={disabled} onChange={e => setDraft(e.target.value)} onBlur={commit} onKeyDown={e => { if (e.key === "Enter") e.currentTarget.blur(); }}/><span className="unit">{unit}</span></span>;
}
function App() {
  const [page, setPage] = useState<Page>("nodes");
  const [setTab, setSetTab] = useState<SettingsTab>("general");
  const [nodes, setNodes] = useState<ProxyNode[]>([]);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [settings, setSettings] = useState<AppSettingsView | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState("");
  const [folded, setFolded] = useState(true);
  const [editorId, setEditorId] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [form, setForm] = useState<NodeInput>(BASE_FORM);
  const [importUrl, setImportUrl] = useState("");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [logFilter, setLogFilter] = useState<LogFilter>("all");
  const [logEntries, setLogEntries] = useState<LogEntry[]>([]);
  const [cookieDraft, setCookieDraft] = useState("");
  const [wirePaths, setWirePaths] = useState<{ d: string; color: string; dashed?: boolean }[]>([]);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [flash, setFlash] = useState<{ text: string; tone: Tone } | null>(null);
  const [theme, setTheme] = useState<Theme>(() => (localStorage.getItem("socksnode.theme.v3") as Theme) || (localStorage.getItem("socksnode.theme") as Theme) || "system");
  const logIdRef = useRef<string | null>(null);
  const toastTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const activeRef = useRef(true);

  const toast = useCallback((text: string, tone: Tone = "ok") => {
    setFlash({ text, tone });
    if (toastTimer.current) clearTimeout(toastTimer.current);
    toastTimer.current = setTimeout(() => setFlash(null), 3200);
  }, []);

  const refresh = useCallback(async () => {
    const list = await api.list();
    if (!activeRef.current) return;
    setNodes(list);
    setSelectedId(prev => list.some(n => n.id === prev) ? prev : (list[0]?.id ?? null));
  }, []);

  useEffect(() => {
    let mounted = true;
    activeRef.current = true;
    let unChanged = () => {};
    let unLog = () => {};
    void Promise.all([api.list(), api.getInfo(), api.getSettings()]).then(([list, metadata, view]) => {
      if (!mounted) return;
      setNodes(list);
      setInfo(metadata);
      setSettings(view);
      setSelectedId(list[0]?.id ?? null);
    }).catch(e => { if (mounted) toast("初始化失败：" + errMessage(e), "err"); }).finally(() => { if (mounted) setLoading(false); });
    unChanged = api.onChanged(list => {
      if (!activeRef.current) return;
      setNodes(list);
      setSelectedId(prev => list.some(n => n.id === prev) ? prev : (list[0]?.id ?? null));
    });
    unLog = api.onLog(e => {
      if (logIdRef.current !== e.id || !activeRef.current) return;
      setLogEntries(current => [...current, { ts: e.ts, line: e.line }].slice(-500));
    });
    return () => { mounted = false; activeRef.current = false; unChanged(); unLog(); if (toastTimer.current) clearTimeout(toastTimer.current); };
  }, [toast]);

  useEffect(() => {
    logIdRef.current = selectedId;
    if (!selectedId) { setLogEntries([]); return; }
    let active = true;
    void api.logs(selectedId).then(data => { if (active) setLogEntries(data); }).catch(() => { if (active) setLogEntries([]); });
    return () => { active = false; };
  }, [selectedId]);

  useEffect(() => {
    const root = document.documentElement;
    const apply = () => { const dark = theme === "dark" || (theme === "system" && window.matchMedia("(prefers-color-scheme: dark)").matches); root.dataset.theme = dark ? "dark" : "light"; };
    apply();
    localStorage.setItem("socksnode.theme.v3", theme);
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    query.addEventListener("change", apply);
    return () => query.removeEventListener("change", apply);
  }, [theme]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === ",") { e.preventDefault(); setPage("settings"); }
      if (e.key === "Escape") { setConfirmDelete(null); setAdding(false); setEditorId(null); }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (page !== "routing") return;
    const flow = document.getElementById("flow");
    const src = document.getElementById("src");
    const hub = document.getElementById("hub");
    const ips = document.getElementById("ips");
    if (!flow || !src || !hub || !ips) return;
    const update = () => {
      const base = flow.getBoundingClientRect();
      const a = src.getBoundingClientRect();
      const b = hub.getBoundingClientRect();
      const p = (x: number, y: number) => [x - base.left, y - base.top];
      const [x1,y1] = p(a.right, a.top + a.height / 2);
      const [x2,y2] = p(b.left, b.top + b.height / 2);
      const [x3,y3] = p(b.right, b.top + b.height / 2);
      const paths = [{ d: "M" + x1 + " " + y1 + " C" + (x1 + 24) + " " + y1 + " " + (x2 - 24) + " " + y2 + " " + x2 + " " + y2, color: "var(--c1)", dashed: true }];
      ips.querySelectorAll(".ipr").forEach((item, index) => {
        const rect = item.getBoundingClientRect();
        const [x4,y4] = p(rect.left, rect.top + rect.height / 2);
        paths.push({ d: "M" + x3 + " " + y3 + " C" + (x3 + 24) + " " + y3 + " " + (x4 - 24) + " " + y4 + " " + x4 + " " + y4, color: index === 0 ? "var(--c1)" : "var(--line)", dashed: index === 0 });
      });
      setWirePaths(paths);
    };
    const frame = requestAnimationFrame(update);
    const observer = new ResizeObserver(update);
    observer.observe(flow);
    return () => { cancelAnimationFrame(frame); observer.disconnect(); };
  }, [page, selectedId, nodes]);

  const selected = nodes.find(n => n.id === selectedId) ?? null;
  const running = nodes.filter(n => n.status === "running");
  const errors = nodes.filter(n => n.status === "error");
  const inProgress = !!busy;

  const perform = async (title: string, fn: () => Promise<unknown>, success?: string): Promise<boolean> => {
    if (inProgress) return false;
    setBusy(title);
    try {
      await fn();
      await refresh();
      if (success) toast(success);
      return true;
    } catch (e) { toast(errMessage(e), "err"); try { await refresh(); } catch { /* retain view */ } return false; }
    finally { setBusy(""); }
  };

  const changePage = (next: Page) => { setPage(next); setAdding(false); setEditorId(null); };
  const openAdd = () => {
    setForm({ ...BASE_FORM, localPort: nextPort(nodes) }); setImportUrl(""); setAdding(true); setEditorId(null); setPage("upstreams");
  };
  const openEdit = (node: ProxyNode) => { setForm(toInput(node)); setEditorId(node.id); setAdding(false); setPage("upstreams"); };
  const onField = <K extends keyof NodeInput>(key: K, value: NodeInput[K]) => setForm(old => ({ ...old, [key]: value }));

  const saveNode = async (startAfter = false) => {
    if (!form.name.trim() || !form.server.trim() || !form.password.trim()) { toast("请填写名称、服务器和密码", "warn"); return; }
    if (!Number.isInteger(form.port) || form.port < 1 || form.port > 65535 || !Number.isInteger(form.localPort) || form.localPort < 1 || form.localPort > 65535) { toast("端口应在 1–65535 之间", "warn"); return; }
    const edit = editorId;
    await perform(edit ? "正在更新节点…" : "正在创建节点…", async () => {
      const before = nodes.map(n => n.id);
      if (edit) await api.update(edit, form);
      else await api.create(form);
      setAdding(false); setEditorId(null);
      if (startAfter) {
        const now = await api.list();
        const created = now.find(n => !before.includes(n.id));
        if (created) { setSelectedId(created.id); await api.start(created.id); setPage("nodes"); }
      }
    }, edit ? "节点已更新" : "节点已添加");
  };

  const applySettings = async (patch: AppSettingsPatch): Promise<boolean> => {
    return perform("正在保存设置…", async () => {
      const result = await api.updateSettings(patch);
      setSettings(result);
    }, "设置已保存");
  };
  const testIlatency = async () => {
    await perform("正在验证 iLatency…", async () => {
      const user = await api.testIlatency(cookieDraft.trim() || undefined);
      toast(cookieDraft.trim() ? "认证有效：" + user + "，请点击保存 Cookie" : "认证有效：" + user);
    });
  };
  const copy = async (value: string, success = "已复制") => {
    try { await navigator.clipboard.writeText(value); toast(success); }
    catch { toast("无法复制到剪贴板", "err"); }
  };
  const nodeAction = (node: ProxyNode, action: "start" | "stop" | "test" | "reselect") => {
    setSelectedId(node.id);
    if (action === "start") void perform("正在启动 " + node.name + "…", () => api.start(node.id), node.name + " 已启动");
    if (action === "stop") void perform("正在停止 " + node.name + "…", () => api.stop(node.id), node.name + " 已停止");
    if (action === "test") void perform("正在测试 " + node.name + "…", async () => {
      const result = await api.test(node.id);
      if (!result.ok) throw new Error(result.message);
      toast(node.name + " · " + result.ms + " ms");
    });
    if (action === "reselect") void perform("正在重新优选 IP…", () => api.reselect(node.id), "IP 优选已完成");
  };
  const allAction = (action: "start" | "stop" | "test") => {
    if (action === "start") void perform("正在批量启动…", () => api.startAll(), "批量启动完成");
    if (action === "stop") void perform("正在批量停止…", () => api.stopAll(), "全部停止");
    if (action === "test") void perform("正在逐个测延迟…", async () => {
      const candidates = nodes.filter(n => n.status === "running");
      if (!candidates.length) throw new Error("目前没有运行中的节点");
      let success = 0;
      for (const node of candidates) { try { const result = await api.test(node.id); if (result.ok) success++; } catch { /* continue */ } }
      toast("延迟测试：" + success + "/" + candidates.length + " 成功", success === candidates.length ? "ok" : "warn");
    });
  };

  const NodeRow = ({ node }: { node: ProxyNode }) => {
    const transitional = node.status === "starting" || node.status === "stopping";
    const active = node.status === "running";
    return <div className={cn("row", !node.enabled && "off")}>
      <span className="ic">{flag(node.name)}</span>
      <div className="who">
        <div className="name">{node.name}</div>
        <div className={cn("sub", node.status === "error" && "err")}>
          <span className={cn("dot", dotClass(node.status))} />
          {node.status === "error" ? <>启动失败 · {node.lastError || "未知错误"}</> :
            transitional ? (node.status === "starting" ? "启动中 · 解析与优选…" : "停止中…") :
            active ? <>已监听 · <span className="mono">127.0.0.1:{node.localPort}</span> {node.lanAccess && " · 局域网可用"} {runtimeLatency(node) != null && <span className={latClass(runtimeLatency(node))}> · {runtimeLatency(node)} ms</span>}</> :
            <>已停止 · 打开后监听 <span className="mono">:{node.localPort}</span>{!node.enabled && " · 不参与批量启动"}</>}
        </div>
      </div>
      {node.preferFastestIp && <button className="count" onClick={() => { setSelectedId(node.id); changePage("routing"); }}>优选详情 <ChevronRight size={13} /></button>}
      <div className="fields"><button className="field" onClick={() => { setSelectedId(node.id); changePage("routing"); }} title="查看此节点的 IP 优选状态">
        <span className={cn("v", "mono", !node.activeIp && "dim")}>{active ? (node.activeIp || node.server) : (node.preferFastestIp ? "启动时优选" : "固定解析")}</span><span className="chev"><ChevronDown size={12} /></span>
      </button></div>
      <Toggle checked={active} label={active ? "停止 " + node.name : "启动 " + node.name} busy={transitional} disabled={inProgress} onClick={() => nodeAction(node, active ? "stop" : "start")} />
    </div>;
  };

  const renderNodes = () => {
    if (loading) return <div className="list">{[1,2,3,4,5].map(i => <div className="row" key={i}><span className="sk" style={{ width: 22, height: 22 }} /><div className="who"><span className="sk" style={{ width: 130, height: 12 }} /><span className="sk" style={{ width: 240, height: 9, marginTop: 7 }} /></div></div>)}</div>;
    if (!nodes.length) return <Empty title="还没有节点">前往「上游」添加 AnyTLS 节点后，即可创建本机 SOCKS5 端口。<br/><TextBtn kind="primary" onClick={openAdd}><Plus size={13}/> 添加节点</TextBtn></Empty>;
    const main = nodes.filter(n => n.enabled), rest = nodes.filter(n => !n.enabled);
    return <><p className="lede">打开开关，节点就会把上游 anytls 转成一个本机 SOCKS5 端口；<b>已监听</b>只代表端口就绪，真实连通请看延迟。所有状态与 IP 均来自运行中的客户端。</p>
      <div className="list">{main.map(n => <NodeRow key={n.id} node={n}/>)}
        {rest.length > 0 && (folded ? <button className="more-row" onClick={() => setFolded(false)}>显示其余 {rest.length} 个（不参与全部启动） <ChevronDown size={12}/></button> : <>{rest.map(n => <NodeRow key={n.id} node={n}/>)}<button className="more-row" onClick={() => setFolded(true)}>收起</button></>)}
      </div>
      <section className="profiles"><span className="label">批量</span><div className="chips">
        <button className="chip" disabled={inProgress} onClick={() => allAction("start")}><span className="lib"/>全部启动</button>
        <button className="chip" disabled={inProgress} onClick={() => allAction("stop")}>全部停止</button>
        <button className="chip" disabled={inProgress} onClick={() => allAction("test")}>全部测延迟</button>
      </div><span className="grow"/><TextBtn onClick={openAdd}><Plus size={13}/> 添加节点</TextBtn></section></>;
  };

  const renderEditor = (isNew: boolean) => {
    const check = (key: keyof NodeInput, label: string) => <button type="button" key={key} className={cn("ck", Boolean(form[key]) && "on")} onClick={() => onField(key, !form[key] as never)}><i />{label}</button>;
    return <div className="editor">
      {!isNew && <div className="ehead"><span className="ic">{flag(form.name)}</span><b>{form.name}</b><span className="note">anytls · 修改运行中的节点会自动重启</span></div>}
      <Field label="名称"><input className="inp" value={form.name} placeholder="例如 香港 01" onChange={e => onField("name", e.target.value)} /></Field>
      <Field label="服务器"><div className="pair"><input className="inp mono" value={form.server} placeholder="域名或 IP" onChange={e => onField("server", e.target.value)} /><input className="inp mono" type="number" min="1" max="65535" aria-label="远程端口" value={form.port} onChange={e => onField("port", Number(e.target.value))}/></div></Field>
      <Field label="密码"><input className="inp mono" type="password" autoComplete="off" value={form.password} onChange={e => onField("password", e.target.value)}/></Field>
      <Field label="SNI"><input className="inp mono" value={form.sni ?? ""} onChange={e => onField("sni", e.target.value)}/></Field>
      <Field label="本机端口"><div><input className="inp mono" style={{ width: 115 }} type="number" min="1" max="65535" value={form.localPort} onChange={e => onField("localPort", Number(e.target.value))}/><div className="hint">127.0.0.1 监听；局域网连入时为 0.0.0.0</div></div></Field>
      <Field label="选项"><div><div className="checks">{check("udp", "UDP over TCP")}{check("preferFastestIp", "启动时优选 IP")}{check("skipCertVerify", "跳过证书验证")}{check("enabled", "参与全部启动")}{check("lanAccess", "局域网连入")}</div>
        {form.lanAccess && <div className="hint warn">局域网代理没有认证，同一网络中的其他设备可能使用它。</div>}
        {form.skipCertVerify && <div className="hint warn">已关闭上游 TLS 证书验证，请仅在可信任的节点上使用。</div>}
      </div></Field>
      <div className="efoot">
        {!isNew && <><TextBtn kind="danger" disabled={inProgress} onClick={() => setConfirmDelete(editorId)}>删除节点</TextBtn>
        <TextBtn onClick={() => { const n = nodes.find(n => n.id === editorId); if (n) void copy(anytlsLink(n), "包含密码的节点链接已复制"); }}>复制为 anytls:// 链接</TextBtn></>}
        <span className="grow" />
        <TextBtn kind="action" onClick={() => { setAdding(false); setEditorId(null); }}>取消</TextBtn>
        {isNew && <TextBtn kind="action" disabled={inProgress} onClick={() => void saveNode(true)}>保存并启动</TextBtn>}
        <TextBtn kind="primary" disabled={inProgress} onClick={() => void saveNode(false)}>保存</TextBtn>
      </div>
    </div>;
  };

  const renderAdd = () => <div>
    <div className="sheet-head"><b>添加节点</b><span className="grow"/><TextBtn onClick={() => setAdding(false)}>关闭 <X size={13}/></TextBtn></div>
    <RowHead label="链接 · 粘贴 AnyTLS URI 自动填写下面的表单"/>
    <div className="uri-card"><Copy size={14} style={{ color: "var(--faint)" }}/><input type="text" value={importUrl} onChange={e => setImportUrl(e.target.value)} placeholder="anytls://密码@主机:端口?sni=www.icloud.com#香港01"/>
      <TextBtn onClick={() => { try { setForm(current => ({ ...current, ...readAnytlsLink(importUrl) })); toast("节点信息已解析，请检查后保存"); } catch(e) { toast(errMessage(e), "err"); } }}>解析</TextBtn>
      <TextBtn onClick={() => void navigator.clipboard.readText().then(value => { setImportUrl(value); setForm(current => ({ ...current, ...readAnytlsLink(value) })); toast("已从剪贴板导入"); }).catch(e => toast(errMessage(e), "err"))}>从剪贴板</TextBtn>
    </div>
    <RowHead label="SNI 预设 · 上游没有指定时使用"/>
    <div className="grid">{SNI_PRESETS.map(sni => <button className={cn("pcard", form.sni === sni && "on")} key={sni} onClick={() => onField("sni", sni)}><span className="ic">🔒</span><span className="who"><span className="name mono">{sni}</span><span className="sub">TLS Server Name</span></span></button>)}</div>
    <RowHead label="手动填写"/>
    <div className="list">{renderEditor(true)}</div>
    <div style={{ height: 16 }} />
  </div>;

  const renderUpstreams = () => {
    if (adding) return renderAdd();
    if (!nodes.length) return <Empty title="还没有上游">手动填写服务器信息，或粘贴 anytls:// 链接。<br/><TextBtn kind="primary" onClick={openAdd}><Plus size={13}/> 添加节点</TextBtn></Empty>;
    return <><div className="list" style={{ marginTop: 7 }}>{nodes.map(n => <div key={n.id}>
      <div className="row">
        <span className="ic">{flag(n.name)}</span>
        <div className="who"><div className="name">{n.name}</div><div className="sub"><span className="mono">{n.server}:{n.port}</span> · SNI {n.sni}</div></div>
        <div className="agent-chips">{n.udp && <span>UDP</span>}{n.preferFastestIp && <span>优选</span>}{n.lanAccess && <span>局域网</span>}</div>
        <span className="secret" style={{ marginLeft: 12 }}><span className={cn("dot", dotClass(n.status))}/>••••••</span>
        <button className="icon" title={editorId === n.id ? "关闭编辑" : "编辑节点"} aria-label={"编辑 " + n.name} onClick={() => editorId === n.id ? setEditorId(null) : openEdit(n)}>
          <ChevronRight size={15} style={{ transform: editorId === n.id ? "rotate(90deg)" : "", transition: "transform .15s" }}/></button>
      </div>
      {editorId === n.id && renderEditor(false)}
    </div>)}</div>
    <div className="after-list"><TextBtn onClick={openAdd}><Plus size={13}/> 添加节点</TextBtn><span className="grow"/><span className="note">修改节点会立即保存到本地配置</span></div></>;
  };

  const renderGateway = () => {
    if (!selected) return <Empty title="没有可用的 SOCKS 端口">添加节点后，这里会显示本机代理地址和连接方法。</Empty>;
    const address = "127.0.0.1:" + selected.localPort;
    const proxy = "socks5h://" + address;
    const commands: [string, string][] = [
      ["终端", "export ALL_PROXY=" + proxy],
      ["curl", "curl -x " + proxy + " https://cp.cloudflare.com"],
      ["浏览器", "SOCKS5 主机：127.0.0.1    端口：" + selected.localPort],
    ];
    return <><div className="list" style={{ marginTop: 7 }}>
      <div className="row gw"><span className="ic" style={{ width: 28 }}><Network size={21}/></span><div className="who"><div className="big">127.0.0.1 <span style={{ color: "var(--muted)", fontWeight: 400 }}>:{selected.localPort}</span></div><div className="sub"><span className={cn("dot", dotClass(selected.status))}/>{running.length} 个端口在监听 · SOCKS5 · 不改系统代理，不开 TUN</div></div></div></div>
      <RowHead label="接入" right={<select className="field" aria-label="选择网关节点" value={selected.id} onChange={e => setSelectedId(e.target.value)}>{nodes.map(n => <option key={n.id} value={n.id}>{n.name} · :{n.localPort}</option>)}</select>}/>
      <div className="list">{commands.map(([title, value]) => <div key={title} className="row cn"><span className="k">{title}</span><div className="codebox">{value}</div><button className="copy" title={"复制 " + title} onClick={() => void copy(value)}><Copy size={14}/></button></div>)}
        <div className="row cn"><span className="k">局域网</span><div className="codebox">{selected.lanAccess ? selected.lanSocks || "已开启局域网监听，当前没有可用的局域网地址" : "未开放 · 默认仅本机可用"}</div>{selected.lanSocks && <button className="copy" title="复制局域网地址" onClick={() => void copy(selected.lanSocks!)}><Copy size={14}/></button>}</div>
        <div className="row"><span className="ic"><span className={cn("dot", dotClass(selected.status))}/></span><div className="who"><div className="name">{selected.name}</div><div className="sub">AnyTLS · {selected.server}:{selected.port} · 监听 :{selected.localPort}</div></div><TextBtn kind="action" disabled={inProgress} onClick={() => nodeAction(selected, "test")}>测延迟</TextBtn></div>
      </div>
      <RowHead label="其他本机端口"/>
      <div className="list">{nodes.map(n => <div className="row" key={n.id}><span className="ic">{flag(n.name)}</span><div className="who"><div className="name">{n.name}</div><div className="sub"><span className={cn("dot", dotClass(n.status))}/><span className="mono">{n.socks}</span></div></div><button className="copy" title="复制 SOCKS 地址" onClick={() => void copy(n.socks)}><Copy size={14}/></button></div>)}</div>
    </>;
  };

  const renderRouting = () => {
    const candidates = nodes.filter(n => n.preferFastestIp);
    const node = selected && selected.preferFastestIp ? selected : candidates[0];
    if (!node) return <Empty title="尚无启用 IP 优选的节点">在「上游」编辑节点并开启「启动时优选 IP」。</Empty>;
    const value = node.activeIp;
    return <>
      <div className="usage-head" style={{ justifyContent: "space-between" }}>
        <select className="field" value={node.id} aria-label="选择优选节点" onChange={e => setSelectedId(e.target.value)}>{candidates.map(n => <option value={n.id} key={n.id}>{n.name} · :{n.localPort}</option>)}</select>
        <span className="grow"/><TextBtn kind="action" disabled={inProgress || node.status !== "running"} onClick={() => nodeAction(node, "reselect")}><RotateCcw size={13}/> 重新优选</TextBtn>
        <TextBtn onClick={() => openEdit(node)}>编辑节点</TextBtn>
      </div>
      <section className="rt">
        <div className="rt-title"><b>{node.name}</b> <span>· 当前上游 IP · {node.status === "running" ? "运行中" : node.status === "error" ? "启动失败" : "未运行"}</span></div>
        <div className="rt-desc">多地 DNS → TCPing → 根据全局策略选择出口。下方显示应用实际记录的 IP 与探测结果，不使用演示候选 IP。</div>
        <div className="rt-flow" id="flow">
          <svg className="wires" aria-hidden="true">{wirePaths.map((path,index) => <path key={index} d={path.d} fill="none" stroke={path.color} strokeWidth={1.5} strokeDasharray={path.dashed ? "3 4" : undefined} />)}</svg>
          <div className="rt-src" id="src"><div className="ic">💻</div><div><div className="n"><span className={cn("dot", dotClass(node.status))}/>本机应用</div><div className="s">socks5h://:{node.localPort}</div></div></div>
          <div className="rt-hub" id="hub"><Network size={22}/><div className="n">SocksNode</div><div className="s">{node.server}</div><span className="badge">{settings ? STRATEGIES.find(([k]) => k === settings.selectStrategy)?.[1] || "自动优选" : "自动优选"}</span></div>
          <div className="rt-ips" id="ips">
            <div className={cn("ipr", value && "use")}><span className="o"/><span className="ip">{value || "尚未选定上游 IP"}</span><span className="r">{node.lastProbe?.ms != null ? node.lastProbe.ms + " ms" : "—"}</span></div>
            <div className="ipr"><span className="o"/><span className="ip">{node.server}</span><span className="r">原始服务器</span></div>
            {!value && <div className="ipr bad"><span className="o"/><span className="ip">启动节点后展示优选结果</span></div>}
          </div>
        </div>
        <div className="rt-foot">
          <div className="say"><strong>{value ? "当前生效：" + value : "等待选定出口"}</strong><div className="sub">{node.lastProbe?.at ? "最近探测：" + new Date(node.lastProbe.at).toLocaleString() : "尚无探测记录"}</div></div>
          <div className="kpi"><b>{node.lastProbe?.ms ?? "—"}</b><span>最近 TCPing · ms</span></div>
          <div className="kpi"><b>{node.lastProbe ? (node.lastProbe.ok ? "正常" : "失败") : "—"}</b><span>探测结果</span></div>
        </div>
        <div className="rt-why"><h5>选择说明</h5><ul><li className="hi">运行时选定的上游地址会保存在节点记录中，停止后可能保留为下次启动的参考。</li><li>当前应用只公开最终 IP 和最后一次探测；多候选排名和测速记录尚未提供给前端。</li>{node.lastProbe?.message && <li>{node.lastProbe.message}</li>}</ul></div>
      </section>
      <RowHead label="支持优选的节点" right={<span className="note">点击切换详情</span>}/>
      <div className="list">{candidates.map(n => <div className="row" key={n.id}><span className="ic">{flag(n.name)}</span><div className="who"><div className="name">{n.name}<span className="tagc">:{n.localPort}</span></div><div className="sub">{n.activeIp || "尚未优选"} · {n.status === "running" ? "运行中" : "未运行"}</div></div><TextBtn onClick={() => setSelectedId(n.id)}>查看 <ChevronRight size={13}/></TextBtn></div>)}</div>
    </>;
  };

  const renderLatency = () => {
    const samples = nodes.map(n => ({ node: n, ms: runtimeLatency(n) })).filter(x => x.ms != null);
    const average = samples.length ? Math.round(samples.reduce((acc, s) => acc + (s.ms || 0), 0) / samples.length) : null;
    const max = Math.max(1, ...samples.map(s => s.ms || 0));
    return <>
      <div className="usage-head"><span className="label">当前快照</span><span className="grow"/><span className="total">{average == null ? "—" : average + " ms"}<small>已测节点平均延迟</small></span></div>
      <div className="qcards">{(running.length ? running : nodes.slice(0,3)).map(n => <div className="qcard" key={n.id}><div className="h"><span className="ic">{flag(n.name)}</span>{n.name}</div><div className="b"><span>{n.status === "running" ? "最近测得" : "未运行"}</span><b className={latClass(runtimeLatency(n))}>{runtimeLatency(n) == null ? "—" : runtimeLatency(n) + " ms"}</b></div></div>)}</div>
      <div className="stats"><div className="stat"><b>{running.length}</b><div className="l">运行中</div><div className="d">共 {nodes.length} 个节点</div></div>
        <div className="stat"><b>{samples.length}</b><div className="l">已有探测数据</div><div className="d">延迟或 TCPing 快照</div></div>
        <div className="stat"><b>{errors.length}</b><div className="l">启动异常</div><div className="d">需要检查日志</div></div>
        <div className="stat"><b>{average ?? "—"}</b><div className="l">平均 ms</div><div className="d">仅统计已有结果</div></div>
      </div>
      <RowHead label="按节点" right={<TextBtn kind="action" disabled={inProgress} onClick={() => allAction("test")}><Activity size={13}/> 全部测延迟</TextBtn>}/>
      {nodes.length ? <div className="list">{nodes.map(n => { const ms = runtimeLatency(n); return <div className="row stat-row" key={n.id}>
        <div className="who" style={{ display: "flex", gap: 10, alignItems: "center" }}><span className="ic">{flag(n.name)}</span><span className="name">{n.name}</span></div>
        <div className="share"><i style={{ width: ms == null ? 0 : Math.max(3, (ms / max) * 100) + "%" }}/></div>
        <div className="nums"><b className={latClass(ms)}>{ms == null ? "—" : ms + " ms"}</b><small>{n.lastTest?.ok ? "SOCKS 测试" : n.lastProbe?.ms != null ? "TCPing" : "未测"}</small></div>
        <TextBtn disabled={inProgress || n.status !== "running"} onClick={() => nodeAction(n, "test")}>测试</TextBtn>
      </div>; })}</div> : <Empty title="没有延迟数据">添加节点并启动后可进行延迟测试。</Empty>}
      <p className="lede" style={{ marginTop: 12 }}>目前后端提供的是最近一次探测和测试结果，未保存 7 天或 30 天的历史时序数据，因此这里不展示虚构的历史图表。</p>
    </>;
  };

  const renderLogs = () => {
    const classify = (text: string): "ok" | "warn" | "err" | "info" => /\b(error|failed|fail|panic|fatal|拒绝|失败|错误)\b/i.test(text) ? "err" : /\b(warn|timeout|重试|警告|超时)\b/i.test(text) ? "warn" : /\b(ok|success|ready|started|listening|成功|完成|已启动)\b/i.test(text) ? "ok" : "info";
    const filtered = logEntries.filter(e => logFilter === "all" || (logFilter === "ok" ? classify(e.line) === "ok" : ["warn", "err"].includes(classify(e.line))));
    return <>
      <div className="usage-head">
        <select className="field" value={selectedId ?? ""} onChange={e => setSelectedId(e.target.value)} aria-label="选择日志节点">{nodes.map(n => <option key={n.id} value={n.id}>{n.name}</option>)}</select>
        <Segment value={logFilter} onChange={setLogFilter} options={[["all", "全部"], ["ok", "成功"], ["warn", "警告与错误"]]}/>
        <span className="grow"/><TextBtn kind="action" onClick={() => { if (selectedId) void api.logs(selectedId).then(setLogEntries).catch(e => toast(errMessage(e), "err")); }}><RefreshCw size={13}/> 刷新</TextBtn>
      </div>
      <div className="logwrap"><div className="list">{filtered.length ? filtered.map((entry,i) => { const level = classify(entry.line); return <div key={entry.ts + i} className="ln"><span className="ts">{fmtTime(entry.ts)}</span><span className={cn("lv", level)}>{level.toUpperCase()}</span><span>{entry.line}</span></div>; }) : <div className="empty-state"><b>暂无日志</b>节点启动后，DNS、tcping、测速和进程日志会出现在这里。</div>}</div></div>
    </>;
  };

  const renderSettings = () => {
    if (!settings) return <Empty title="正在读取设置">请检查后端服务是否正常运行。</Empty>;
    const s = settings;
    const buttons: [SettingsTab, string][] = [["general", "通用"], ["select", "优选与测速"], ["network", "网络与共享"], ["ilatency", "iLatency"], ["about", "关于"]];
    const number = (value: number, unit: string, min: number, max: number, setter: (value: number) => void) => <NumericSetting value={value} unit={unit} min={min} max={max} disabled={inProgress} onSave={setter} />;
    const boolControl = (value: boolean, field: keyof AppSettingsPatch) => <Segment value={value ? "true" : "false"} options={[["true", "开"], ["false", "关"]]} disabled={inProgress} onChange={v => void applySettings({ [field]: v === "true" })}/>;
    return <>
      <div className="row-head set-head"><div className="warm-tabs">{buttons.map(([key,text]) => <button className={cn(setTab === key && "on")} key={key} onClick={() => setSetTab(key)}>{text}</button>)}</div></div>
      <div className="list">
        {setTab === "general" && <>
          <PrefRow title="外观" description="浅色、深色，或跟随 macOS 系统" control={<Segment value={theme} options={[["system","跟随系统"],["light","浅色"],["dark","深色"]]} onChange={setTheme}/>}/>
          <PrefRow title="登录时打开" description={<>注册 macOS 登录项，开机自动启动 SocksNode{!s.launchAtLoginActive && s.launchAtLogin && <span className="sub err"> · 登录项未生效</span>}</>} control={boolControl(s.launchAtLogin, "launchAtLogin")}/>
          <PrefRow title="启动时最小化" description="登录启动时不弹出主窗口" control={<Segment value={s.startMinimized ? "true" : "false"} options={[["true","开"],["false","关"]]} disabled={inProgress || !s.launchAtLogin} onChange={v => void applySettings({ startMinimized: v === "true" })}/>}/>
          <PrefRow title="打开时启动节点" description="仅启动标记为「参与全部启动」的节点" control={boolControl(s.autoStartNodes, "autoStartNodes")}/>
        </>}
        {setTab === "select" && <>
          <PrefRow title="优选策略" description="多地 DNS → TCPing → Top K 测速 → 选择最快出口" control={<Segment value={s.selectStrategy as SelectStrategy} options={STRATEGIES} disabled={inProgress} onChange={v => void applySettings({ selectStrategy: v })}/>}/>
          <PrefRow title="测速候选数" description="TCPing 最快的前 K 个参与测速，范围 1–8" control={number(s.speedTopK, "个", 1, 8, v => void applySettings({ speedTopK: v }))}/>
          <PrefRow title="测速下载量" description="每个候选的下载量，1–50 MB" control={number(Math.round(s.speedTestBytes / 1048576), "MB", 1, 50, v => void applySettings({ speedTestBytes: v * 1048576 }))}/>
          <PrefRow title="测速超时" description="单个测速请求允许的最长时间" control={number(s.speedTestTimeoutSecs, "秒", 3, 60, v => void applySettings({ speedTestTimeoutSecs: v }))}/>
          <PrefRow title="探活间隔" description="运行中上游健康检查的间隔，3–120 秒" control={number(s.healthIntervalSecs, "秒", 3, 120, v => void applySettings({ healthIntervalSecs: v }))}/>
          <PrefRow title="切换冷却" description="上游故障重新优选后，最短冷却间隔，15–600 秒" control={number(s.failoverCooldownSecs, "秒", 15, 600, v => void applySettings({ failoverCooldownSecs: v }))}/>
        </>}
        {setTab === "network" && <>
          <PrefRow title="默认监听" description="不开局域网连入时，SOCKS5 仅监听本机 127.0.0.1" control={<span className="mono">127.0.0.1</span>}/>
          <PrefRow title="局域网共享" description="可以在「上游」中单独为每个节点打开或关闭" control={<TextBtn kind="action" onClick={() => changePage("upstreams")}>管理节点 <ChevronRight size={13}/></TextBtn>}/>
          <PrefRow title="认证" description="内核暂不支持 SOCKS5 用户名密码。启用局域网接入后，同网段其他设备可以直接使用代理。" control={<span className="pillt" style={{ color: "var(--amber)" }}>不支持</span>}/>
          <PrefRow title="物理出口" description="优先选择物理 IPv4 网卡。网络切换后，正在运行的内核可能需要重启。" control={<span className="note">自动检测</span>}/>
        </>}
        {setTab === "ilatency" && <>
          <PrefRow title="多地 DNS 会话" description={s.hasIlatencyCookie ? "已配置 Cookie · " + s.ilatencyCookiePreview : "未配置时使用本机 DNS 和公共 DoH"} control={<span className={cn("pillt", s.hasIlatencyCookie && "ok-txt")}>{s.hasIlatencyCookie ? "已配置" : "未配置"}</span>}/>
          <div className="row pref"><div className="who"><div className="name">会话 Cookie</div><div className="sub wrap">输入新 Cookie 后可先验证，只有点击保存才会更新本地配置</div></div>
            <input className="inp mono" style={{ width: "min(300px,40%)" }} type="password" autoComplete="off" value={cookieDraft} placeholder={s.hasIlatencyCookie ? "留空保留现有 Cookie" : "nodeping_user_session=..."} onChange={e => setCookieDraft(e.target.value)}/>
            </div>
          <div className="row pref"><span className="grow"/><TextBtn kind="action" disabled={inProgress} onClick={() => void testIlatency()}>测试会话</TextBtn><TextBtn kind="primary" disabled={inProgress || !cookieDraft.trim()} onClick={() => void applySettings({ ilatencyCookie: cookieDraft.trim() }).then(ok => { if (ok) setCookieDraft(""); })}>保存 Cookie</TextBtn></div>
        </>}
        {setTab === "about" && <>
          <PrefRow title="SocksNode" description="Tauri 2 · AnyTLS SOCKS5 桌面管理工具" control={<span className="mono">v{info?.version || "—"}</span>}/>
          <PrefRow title="内置内核" description="项目定制的 AnyTLS-Go 客户端" control={<span className="mono">socksnode5</span>}/>
          <PrefRow title="数据目录" description={<span className="mono">{info?.dataDir || "—"}</span>} control={<TextBtn kind="action" onClick={() => { if (info) void api.openPath(info.dataDir).catch(e => toast(errMessage(e),"err")); }}><FolderOpen size={13}/> 打开</TextBtn>}/>
          <PrefRow title="源代码" description="开源项目仓库" control={<TextBtn kind="action" onClick={() => void api.openPath("https://github.com/ct-jyjntc/SocksNode").catch(e => toast(errMessage(e), "err"))}><GitBranch size={13}/> GitHub</TextBtn>}/>
        </>}
      </div>
      <p className="lede" style={{ marginTop: 12 }}>设置更改直接写入本机配置。节点密码用于 AnyTLS 连接，Cookie 在访问 iLatency 服务时使用。</p>
    </>;
  };

  const renderContent = () => {
    switch (page) {
      case "nodes": return renderNodes();
      case "upstreams": return renderUpstreams();
      case "gateway": return renderGateway();
      case "routing": return renderRouting();
      case "latency": return renderLatency();
      case "logs": return renderLogs();
      case "settings": return renderSettings();
    }
  };

  const closeWindow = (action: "close" | "minimize" | "maximize") => {
    const win = getCurrentWindow();
    const task = action === "close" ? win.close() : action === "minimize" ? win.minimize() : win.toggleMaximize();
    void task.catch(e => toast(errMessage(e), "err"));
  };

  const statusText = flash?.text || (loading ? "正在加载节点…" : busy || running.length + " 个节点在监听 · " + errors.length + " 个异常");
  return <div style={{ display: "flex", flexDirection: "column", width: "100%", height: "100%", minHeight: 0 }}>
    <header className="top">
      <div className="lights">
        <button title="关闭窗口" aria-label="关闭窗口" onClick={() => closeWindow("close")}><i/></button>
        <button title="最小化" aria-label="最小化" onClick={() => closeWindow("minimize")}><i/></button>
        <button title="最大化或还原" aria-label="最大化或还原" onClick={() => closeWindow("maximize")}><i/></button>
      </div>
      <div className="brand"><Network size={18} strokeWidth={2}/><span>SocksNode</span></div>
      <nav className="seg" aria-label="主导航">{NAV.map(([key, label]) => <button key={key} className={cn(page === key && "on", key === "nodes" && errors.length > 0 && "has-dot")} onClick={() => changePage(key)}>{label}</button>)}</nav>
      <div className="actions">
        <IconBtn title="刷新状态" disabled={inProgress} onClick={() => void refresh().then(() => toast("状态已刷新")).catch(e => toast(errMessage(e), "err"))}><RefreshCw size={15}/></IconBtn>
        <IconBtn title="切换主题" onClick={() => setTheme(value => value === "dark" ? "light" : "dark")}>{theme === "dark" ? <Sun size={15}/> : <Moon size={15}/>}</IconBtn>
        <IconBtn title="设置 ⌘," onClick={() => changePage(page === "settings" ? "nodes" : "settings")}><Settings2 size={16}/></IconBtn>
      </div>
    </header>
    <main className={cn("view", !["routing", "latency"].includes(page) && "narrow")} key={page}>{renderContent()}</main>
    <footer className="foot"><span className={cn("status", flash?.tone || (errors.length ? "warn" : ""))}>{flash?.tone === "ok" && <Check size={13} />}{statusText}</span><span className="grow"/><span className="note">{nodes.length} 个节点 · v{info?.version || "1.0.0"}</span></footer>
    {confirmDelete && <div className="modal" onMouseDown={e => { if (e.target === e.currentTarget) setConfirmDelete(null); }}>
      <div className="dialog" role="dialog" aria-modal="true" aria-labelledby="delete-title">
        <b id="delete-title">删除「{nodes.find(n => n.id === confirmDelete)?.name}」？</b>
        <p>节点会先停止，然后删除它的配置及运行日志。此操作无法撤销。</p>
        <div className="btns"><TextBtn kind="action" onClick={() => setConfirmDelete(null)}>取消</TextBtn>
        <TextBtn kind="primary" disabled={inProgress} onClick={() => { const id = confirmDelete; setConfirmDelete(null); void perform("正在删除节点…", async () => { await api.remove(id); setEditorId(null); }, "节点已删除"); }}>确认删除</TextBtn></div>
      </div>
    </div>}
  </div>;
}

export default App;
