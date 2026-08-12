import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import {
  Activity,
  FolderOpen,
  Moon,
  Play,
  Plus,
  Square,
  Sun,
  Terminal,
  Trash2,
  Pencil,
  Copy,
  RefreshCw,
  Settings,
} from "lucide-react";
import {
  Badge,
  Button,
  Checkbox,
  Dialog,
  Input,
  Label,
  Spinner,
} from "@/components/ui";
import { WindowControls } from "@/components/WindowControls";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "@/lib/api";
import type {
  AppInfo,
  AppSettingsView,
  LogEntry,
  NodeInput,
  ProxyNode,
} from "@/lib/types";
import { cn, statusLabel } from "@/lib/utils";

const emptyForm: NodeInput = {
  name: "",
  type: "anytls",
  server: "",
  port: 443,
  password: "",
  udp: true,
  sni: "www.icloud.com",
  skipCertVerify: true,
  localPort: 5003,
  preferFastestIp: true,
  lanAccess: false,
};

function useTheme() {
  const [dark, setDark] = useState(() => {
    if (typeof window === "undefined") return false;
    const saved = localStorage.getItem("socksnode.theme");
    if (saved === "dark") return true;
    if (saved === "light") return false;
    return window.matchMedia("(prefers-color-scheme: dark)").matches;
  });

  useEffect(() => {
    document.documentElement.classList.toggle("dark", dark);
    localStorage.setItem("socksnode.theme", dark ? "dark" : "light");
  }, [dark]);

  return { dark, setDark };
}

function statusTone(status: string): "success" | "danger" | "muted" | "warn" {
  if (status === "running") return "success";
  if (status === "error") return "danger";
  if (status === "starting" || status === "stopping") return "warn";
  return "muted";
}

function StatusDot({ status }: { status: string }) {
  return (
    <span
      className={cn(
        "mt-0.5 size-1.5 shrink-0 rounded-full",
        status === "running" && "bg-success",
        status === "error" && "bg-destructive",
        (status === "starting" || status === "stopping") &&
          "animate-pulse bg-amber-500",
        status !== "running" &&
          status !== "error" &&
          status !== "starting" &&
          status !== "stopping" &&
          "bg-muted-foreground/40",
      )}
      aria-hidden
    />
  );
}

export default function App() {
  const { dark, setDark } = useTheme();
  const [nodes, setNodes] = useState<ProxyNode[]>([]);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [busyAction, setBusyAction] = useState<
    "start" | "stop" | "remove" | "test" | "reselect" | null
  >(null);
  const [globalBusy, setGlobalBusy] = useState(false);
  const [globalBusyLabel, setGlobalBusyLabel] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const [settingsBusy, setSettingsBusy] = useState(false);
  const [settingsBusyLabel, setSettingsBusyLabel] = useState("");
  const [dialogOpen, setDialogOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [editing, setEditing] = useState<ProxyNode | null>(null);
  const [form, setForm] = useState<NodeInput>(emptyForm);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [query, setQuery] = useState("");
  const [settings, setSettings] = useState<AppSettingsView | null>(null);
  const [cookieDraft, setCookieDraft] = useState("");
  const [healthInterval, setHealthInterval] = useState(5);
  const [failoverCooldown, setFailoverCooldown] = useState(60);
  const [selectStrategy, setSelectStrategy] = useState("throughput");
  const [speedTopK, setSpeedTopK] = useState(3);
  const [speedTestMb, setSpeedTestMb] = useState(1);
  const [launchAtLogin, setLaunchAtLogin] = useState(false);
  const [autoStartNodes, setAutoStartNodes] = useState(false);
  const selectedIdRef = useRef<string | null>(null);
  const logEndRef = useRef<HTMLDivElement | null>(null);
  const flashTimerRef = useRef<number | null>(null);
  const [flash, setFlash] = useState<{
    text: string;
    tone: "success" | "danger" | "muted";
  } | null>(null);

  const notify = useCallback(
    (text: string, tone: "success" | "danger" | "muted" = "muted") => {
      setFlash({ text, tone });
      if (flashTimerRef.current != null) {
        window.clearTimeout(flashTimerRef.current);
      }
      flashTimerRef.current = window.setTimeout(() => {
        setFlash(null);
        flashTimerRef.current = null;
      }, 3600);
    },
    [],
  );

  useEffect(() => {
    selectedIdRef.current = selectedId;
  }, [selectedId]);

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  useEffect(() => {
    return () => {
      if (flashTimerRef.current != null) {
        window.clearTimeout(flashTimerRef.current);
      }
    };
  }, []);

  const refresh = useCallback(async () => {
    setRefreshing(true);
    try {
      const list = await api.list();
      setNodes(list);
    } finally {
      setRefreshing(false);
      setLoading(false);
    }
  }, []);

  const withGlobalBusy = useCallback(
    async <T,>(label: string, fn: () => Promise<T>): Promise<T> => {
      setGlobalBusy(true);
      setGlobalBusyLabel(label);
      await new Promise<void>((r) => requestAnimationFrame(() => r()));
      try {
        return await fn();
      } finally {
        setGlobalBusy(false);
        setGlobalBusyLabel("");
      }
    },
    [],
  );

  useEffect(() => {
    let unsubChanged = () => {};
    let unsubLog = () => {};

    (async () => {
      try {
        const [list, appInfo, appSettings] = await Promise.all([
          api.list(),
          api.getInfo(),
          api.getSettings(),
        ]);
        setNodes(list);
        setInfo(appInfo);
        setSettings(appSettings);
        setHealthInterval(appSettings.healthIntervalSecs);
        setFailoverCooldown(appSettings.failoverCooldownSecs);
        setSelectStrategy(appSettings.selectStrategy || "throughput");
        setSpeedTopK(appSettings.speedTopK || 3);
        setSpeedTestMb(
          Math.max(0.25, (appSettings.speedTestBytes || 1048576) / 1048576),
        );
        setLaunchAtLogin(Boolean(appSettings.launchAtLogin));
        setAutoStartNodes(Boolean(appSettings.autoStartNodes));
        if (list[0]) setSelectedId(list[0].id);
      } catch (err) {
        notify(err instanceof Error ? err.message : "初始化失败", "danger");
      } finally {
        setLoading(false);
      }
    })();

    unsubChanged = api.onChanged((list) => {
      setNodes(list);
    });

    unsubLog = api.onLog((payload) => {
      if (selectedIdRef.current === payload.id) {
        setLogs((prev) => {
          const next = [...prev, { ts: payload.ts, line: payload.line }];
          return next.slice(-500);
        });
      }
    });

    return () => {
      unsubChanged();
      unsubLog();
    };
  }, []);

  useEffect(() => {
    if (!selectedId) {
      setLogs([]);
      return;
    }
    api.logs(selectedId).then(setLogs).catch(() => setLogs([]));
  }, [selectedId]);

  const stats = useMemo(() => {
    const running = nodes.filter((n) => n.status === "running").length;
    const errored = nodes.filter((n) => n.status === "error").length;
    return {
      total: nodes.length,
      running,
      stopped: nodes.length - running - errored,
      errored,
    };
  }, [nodes]);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return nodes;
    return nodes.filter(
      (n) =>
        n.name.toLowerCase().includes(q) ||
        n.server.toLowerCase().includes(q) ||
        String(n.localPort).includes(q),
    );
  }, [nodes, query]);

  const selected = useMemo(
    () => nodes.find((n) => n.id === selectedId) || null,
    [nodes, selectedId],
  );

  const openCreate = () => {
    setEditing(null);
    const used = new Set(nodes.map((n) => n.localPort));
    let port = 5001;
    while (used.has(port)) port += 1;
    setForm({ ...emptyForm, localPort: port });
    setDialogOpen(true);
  };

  const openEdit = (node: ProxyNode) => {
    setEditing(node);
    setForm({
      name: node.name,
      type: node.type,
      server: node.server,
      port: node.port,
      password: node.password,
      udp: node.udp,
      sni: node.sni,
      skipCertVerify: node.skipCertVerify,
      localPort: node.localPort,
      preferFastestIp: node.preferFastestIp !== false,
      lanAccess: Boolean(node.lanAccess),
    });
    setDialogOpen(true);
  };

  const openSettings = async () => {
    try {
      setSettingsBusy(true);
      setSettingsBusyLabel("读取设置…");
      const s = await api.getSettings();
      setSettings(s);
      setCookieDraft("");
      setHealthInterval(s.healthIntervalSecs);
      setFailoverCooldown(s.failoverCooldownSecs);
      setSelectStrategy(s.selectStrategy || "throughput");
      setSpeedTopK(s.speedTopK || 3);
      setSpeedTestMb(Math.max(0.25, (s.speedTestBytes || 1048576) / 1048576));
      setLaunchAtLogin(Boolean(s.launchAtLogin));
      setAutoStartNodes(Boolean(s.autoStartNodes));
      setSettingsOpen(true);
    } catch (err) {
      notify(err instanceof Error ? err.message : "读取设置失败", "danger");
    } finally {
      setSettingsBusy(false);
      setSettingsBusyLabel("");
    }
  };

  const saveSettings = async () => {
    try {
      setSettingsBusy(true);
      setSettingsBusyLabel("保存设置…");
      const patch: {
        ilatencyCookie?: string;
        healthIntervalSecs: number;
        failoverCooldownSecs: number;
        selectStrategy: string;
        speedTopK: number;
        speedTestBytes: number;
        launchAtLogin: boolean;
        autoStartNodes: boolean;
      } = {
        healthIntervalSecs: healthInterval,
        failoverCooldownSecs: failoverCooldown,
        selectStrategy,
        speedTopK,
        speedTestBytes: Math.round(speedTestMb * 1048576),
        launchAtLogin,
        autoStartNodes,
      };
      if (cookieDraft.trim()) {
        patch.ilatencyCookie = cookieDraft.trim();
      }
      const s = await api.updateSettings(patch);
      setSettings(s);
      setCookieDraft("");
      notify("设置已保存", "success");
      setSettingsOpen(false);
    } catch (err) {
      notify(err instanceof Error ? err.message : "保存设置失败", "danger");
    } finally {
      setSettingsBusy(false);
      setSettingsBusyLabel("");
    }
  };

  const testIlatency = async () => {
    try {
      setSettingsBusy(true);
      setSettingsBusyLabel("验证 iLatency 会话…");
      if (cookieDraft.trim()) {
        await api.updateSettings({ ilatencyCookie: cookieDraft.trim() });
      }
      const user = await api.testIlatency();
      const s = await api.getSettings();
      setSettings(s);
      notify(`iLatency 已登录：${user}`, "success");
    } catch (err) {
      notify(err instanceof Error ? err.message : "iLatency 会话无效", "danger");
    } finally {
      setSettingsBusy(false);
      setSettingsBusyLabel("");
    }
  };

  const saveNode = async () => {
    try {
      await withGlobalBusy(editing ? "正在更新节点…" : "正在添加节点…", async () => {
        if (editing) {
          await api.update(editing.id, form);
          notify("节点已更新", "success");
        } else {
          await api.create(form);
          notify("节点已添加", "success");
        }
        setDialogOpen(false);
        await refresh();
      });
    } catch (err) {
      notify(err instanceof Error ? err.message : "保存失败", "danger");
    }
  };

  const run = async (
    id: string,
    action: "start" | "stop" | "remove" | "test" | "reselect",
  ) => {
    const labels = {
      start: "启动中（含 IP 优选/测速）…",
      stop: "停止中…",
      remove: "删除中…",
      test: "测延迟中…",
      reselect: "重新优选 IP 中…",
    } as const;
    try {
      setBusyId(id);
      setBusyAction(action);
      setGlobalBusyLabel(labels[action]);
      if (action === "start" || action === "test" || action === "stop") {
        setSelectedId(id);
        selectedIdRef.current = id;
        try {
          const existing = await api.logs(id);
          setLogs(existing);
        } catch {
          setLogs([]);
        }
      }
      await new Promise<void>((r) => requestAnimationFrame(() => r()));
      if (action === "start") {
        await api.start(id);
        try {
          setLogs(await api.logs(id));
        } catch {
          /* ignore */
        }
        const node = (await api.list()).find((n) => n.id === id);
        if (node?.activeIp) {
          notify(
            `已启动 · 上游 ${node.activeIp}${
              node.lastProbe?.ms != null ? ` · ${node.lastProbe.ms}ms` : ""
            }`,
            "success",
          );
        } else {
          notify("本地 SOCKS 已启动", "success");
        }
        await refresh();
      } else if (action === "stop") {
        await api.stop(id);
        notify("节点已停止", "success");
        await refresh();
      } else if (action === "test") {
        const result = await api.test(id);
        if (result.ok) {
          notify(result.message, "success");
        } else {
          notify(result.message, "danger");
        }
        await refresh();
      } else if (action === "reselect") {
        await api.reselect(id);
        try {
          setLogs(await api.logs(id));
        } catch {
          /* ignore */
        }
        notify("已触发重新优选 IP", "success");
        await refresh();
      } else {
        await api.remove(id);
        notify("节点已删除", "success");
        if (selectedId === id) setSelectedId(null);
        await refresh();
      }
    } catch (err) {
      notify(err instanceof Error ? err.message : "操作失败", "danger");
      try {
        if (selectedIdRef.current === id) {
          setLogs(await api.logs(id));
        }
      } catch {
        /* ignore */
      }
      await refresh();
    } finally {
      setBusyId(null);
      setBusyAction(null);
      setGlobalBusyLabel("");
    }
  };

  const startAll = async () => {
    try {
      await withGlobalBusy("并行启动中…", async () => {
        await api.startAll();
        notify("全部节点已启动", "success");
      });
    } catch (err) {
      notify(err instanceof Error ? err.message : "批量启动失败", "danger");
    }
  };

  const stopAll = async () => {
    try {
      await withGlobalBusy("并行停止中…", async () => {
        await api.stopAll();
        notify("全部节点已停止", "success");
      });
    } catch (err) {
      notify(err instanceof Error ? err.message : "批量停止失败", "danger");
    }
  };

  const anyBusy =
    globalBusy ||
    !!busyId ||
    refreshing ||
    settingsBusy ||
    nodes.some((n) => n.status === "starting" || n.status === "stopping");
  const busyLabel =
    globalBusyLabel ||
    settingsBusyLabel ||
    (refreshing
      ? "刷新中…"
      : busyId
        ? busyAction === "start"
          ? "节点启动/优选中…"
          : busyAction === "stop"
            ? "节点停止中…"
            : busyAction === "test"
              ? "测延迟中…"
              : busyAction === "reselect"
                ? "重新优选 IP 中…"
                : "处理中…"
        : nodes.some((n) => n.status === "starting")
          ? "节点启动中…"
          : nodes.some((n) => n.status === "stopping")
            ? "节点停止中…"
            : "处理中…");

  // Prefer live busy text; otherwise show the last success/error flash.
  const toolbarStatus = anyBusy
    ? { text: busyLabel, tone: "info" as const, busy: true }
    : flash
      ? { text: flash.text, tone: flash.tone, busy: false }
      : null;

  const copyText = async (text: string, label = "已复制") => {
    try {
      await navigator.clipboard.writeText(text);
      notify(label, "success");
    } catch {
      notify("复制失败", "danger");
    }
  };

  const startDragging = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const el = e.target as HTMLElement;
    if (el.closest("button, a, input, textarea, select, [role=button]")) return;
    e.preventDefault();
    void getCurrentWindow().startDragging();
  };

  const onTitleDoubleClick = (e: React.MouseEvent) => {
    if ((e.target as HTMLElement).closest("button, a, input")) return;
    void getCurrentWindow().toggleMaximize();
  };

  const nodeBusy = selected
    ? busyId === selected.id ||
      selected.status === "starting" ||
      selected.status === "stopping"
    : false;

  return (
    <div className="flex h-full flex-col overflow-hidden bg-background">
      {/* Custom title bar + self-drawn traffic lights (decorations: false) */}
      <header className="flex h-12 shrink-0 items-center border-b border-border/60">
        <WindowControls />
        <div
          data-tauri-drag-region
          className="app-drag flex h-full min-w-0 flex-1 items-center gap-3"
          onMouseDown={startDragging}
          onDoubleClick={onTitleDoubleClick}
        >
          <span className="text-sm font-semibold leading-none tracking-tight">
            SocksNode
          </span>
          <span className="inline-flex items-center gap-1.5 rounded-full bg-secondary/55 px-2 py-0.5 text-[11px] text-muted-foreground">
            <span
              className={cn(
                "size-1.5 rounded-full",
                stats.running > 0 ? "bg-success" : "bg-muted-foreground/40",
              )}
            />
            <span className="tabular-nums">
              {stats.running}/{stats.total}
            </span>
            运行
            {stats.errored > 0 ? (
              <span className="text-destructive">· {stats.errored} 异常</span>
            ) : null}
          </span>
        </div>
        <div className="app-no-drag flex h-full min-w-0 items-center gap-1.5 pr-3">
          {toolbarStatus ? (
            <div
              className={cn(
                "mr-1 flex min-w-0 max-w-[min(360px,36vw)] items-center gap-1.5 text-[11px]",
                toolbarStatus.tone === "success" && "text-success",
                toolbarStatus.tone === "danger" && "text-destructive",
                toolbarStatus.tone === "info" && "text-muted-foreground",
                toolbarStatus.tone === "muted" && "text-muted-foreground",
              )}
              title={toolbarStatus.text}
            >
              {toolbarStatus.busy ? <Spinner size="xs" /> : null}
              <span className="truncate">{toolbarStatus.text}</span>
            </div>
          ) : null}
          <Button
            variant="secondary"
            size="sm"
            className="text-muted-foreground"
            loading={globalBusy && globalBusyLabel.includes("停止")}
            disabled={globalBusy || stats.total === 0}
            onClick={stopAll}
          >
            <Square className="size-3" strokeWidth={1.7} />
            全部停止
          </Button>
          <Button
            size="sm"
            loading={globalBusy && globalBusyLabel.includes("启动")}
            disabled={globalBusy || stats.total === 0}
            onClick={startAll}
          >
            <Play className="size-3" strokeWidth={1.7} />
            全部启动
          </Button>
          <div className="mx-0.5 h-4 w-px bg-border" />
          {info ? (
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label="数据目录"
              disabled={anyBusy}
              onClick={() => api.openPath(info.dataDir)}
            >
              <FolderOpen className="size-3.5" strokeWidth={1.7} />
            </Button>
          ) : null}
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="设置"
            loading={settingsBusy && !settingsOpen}
            onClick={openSettings}
          >
            {!settingsBusy || settingsOpen ? (
              <Settings className="size-3.5" strokeWidth={1.7} />
            ) : null}
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="切换主题"
            onClick={() => setDark(!dark)}
          >
            {dark ? (
              <Sun className="size-3.5" strokeWidth={1.7} />
            ) : (
              <Moon className="size-3.5" strokeWidth={1.7} />
            )}
          </Button>
        </div>
      </header>

      {/* Workspace: node rail + detail/logs */}
      <div className="flex min-h-0 flex-1">
        {/* Left: node list (primary proxy client surface) */}
        <aside className="flex w-[300px] shrink-0 flex-col border-r border-border/60 bg-sidebar">
          <div className="flex shrink-0 items-center gap-1.5 border-b border-border/60 px-2.5 py-2">
            <Input
              className="h-7 bg-background/60"
              placeholder="搜索节点…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            <Button
              variant="ghost"
              size="icon-sm"
              loading={refreshing}
              onClick={refresh}
              aria-label="刷新"
            >
              {!refreshing ? (
                <RefreshCw className="size-3.5" strokeWidth={1.7} />
              ) : null}
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              disabled={globalBusy}
              onClick={openCreate}
              aria-label="添加节点"
            >
              <Plus className="size-3.5" strokeWidth={1.7} />
            </Button>
          </div>

          <div className="min-h-0 flex-1 basis-0 overflow-y-auto py-1">
            {loading ? (
              <div className="space-y-1 px-2 py-2">
                {Array.from({ length: 4 }).map((_, i) => (
                  <div
                    key={i}
                    className="h-12 animate-pulse rounded-md bg-secondary/60"
                  />
                ))}
              </div>
            ) : filtered.length === 0 ? (
              <div className="flex h-full flex-col items-center justify-center gap-2 px-4 text-center">
                <p className="text-xs text-muted-foreground">
                  {nodes.length === 0 ? "还没有节点" : "无匹配结果"}
                </p>
                {nodes.length === 0 ? (
                  <Button size="sm" onClick={openCreate}>
                    <Plus className="size-3" strokeWidth={1.7} />
                    添加节点
                  </Button>
                ) : null}
              </div>
            ) : (
              filtered.map((node) => {
                const active = selectedId === node.id;
                const latency =
                  node.lastProbe?.ms ?? node.lastTest?.ms ?? null;
                return (
                  <button
                    key={node.id}
                    type="button"
                    onClick={() => setSelectedId(node.id)}
                    className={cn(
                      "flex w-full items-start gap-2.5 px-3 py-2.5 text-left transition-colors",
                      "hover:bg-secondary/50",
                      active && "bg-secondary/70",
                    )}
                  >
                    <StatusDot status={node.status} />
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center justify-between gap-2">
                        <span className="truncate text-xs font-medium">
                          {node.name}
                        </span>
                        <span
                          className={cn(
                            "shrink-0 tabular-nums text-[11px]",
                            latency != null
                              ? "text-muted-foreground"
                              : "text-muted-foreground/50",
                          )}
                        >
                          {latency != null ? `${latency}ms` : "—"}
                        </span>
                      </div>
                      <div className="mt-0.5 flex items-center gap-1.5 text-[11px] text-muted-foreground">
                        <span className="font-mono tabular-nums">
                          :{node.localPort}
                        </span>
                        <span className="opacity-40">·</span>
                        <span className="truncate">{node.type}</span>
                        {node.status === "starting" ||
                        node.status === "stopping" ||
                        busyId === node.id ? (
                          <Spinner size="xs" className="ml-auto" />
                        ) : null}
                      </div>
                      {node.lastError ? (
                        <div className="mt-0.5 truncate text-[10px] text-destructive">
                          {node.lastError}
                        </div>
                      ) : null}
                    </div>
                  </button>
                );
              })
            )}
          </div>

          <div className="flex shrink-0 items-center justify-between gap-2 border-t border-border/60 px-3 py-2 text-[11px] text-muted-foreground">
            <span className="tabular-nums">
              {filtered.length}
              {query ? ` / ${nodes.length}` : ""} 节点
            </span>
            {info ? (
              <span className="truncate tabular-nums">v{info.version}</span>
            ) : null}
          </div>
        </aside>

        {/* Right: selected node + logs */}
        <section className="flex min-w-0 flex-1 flex-col">
          {!selected ? (
            <div className="flex flex-1 flex-col items-center justify-center gap-2 text-center">
              <Terminal
                className="size-5 text-muted-foreground/50"
                strokeWidth={1.6}
              />
              <p className="text-xs text-muted-foreground">
                从左侧选择节点，或添加新节点
              </p>
            </div>
          ) : (
            <>
              {/* Node inspector */}
              <div className="shrink-0 border-b border-border/60 px-4 py-3">
                <div className="flex flex-wrap items-start justify-between gap-3">
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-center gap-2">
                      <h2 className="truncate text-sm font-medium">
                        {selected.name}
                      </h2>
                      <Badge tone={statusTone(selected.status)}>
                        {(selected.status === "starting" ||
                          selected.status === "stopping" ||
                          busyId === selected.id) && <Spinner size="xs" />}
                        {statusLabel(selected.status)}
                      </Badge>
                    </div>
                    <p className="mt-1 truncate font-mono text-[11px] text-muted-foreground">
                      {selected.server}:{selected.port}
                      <span className="mx-1 opacity-40">·</span>
                      {selected.type}
                      {selected.sni ? (
                        <>
                          <span className="mx-1 opacity-40">·</span>
                          sni {selected.sni}
                        </>
                      ) : null}
                    </p>
                  </div>

                  <div className="flex flex-wrap items-center gap-1.5">
                    {selected.status === "running" ? (
                      <>
                        <Button
                          variant="outline"
                          size="sm"
                          loading={
                            busyId === selected.id && busyAction === "test"
                          }
                          disabled={nodeBusy || globalBusy}
                          onClick={() => run(selected.id, "test")}
                        >
                          {!(
                            busyId === selected.id && busyAction === "test"
                          ) ? (
                            <Activity className="size-3" strokeWidth={1.7} />
                          ) : null}
                          延迟
                        </Button>
                        <Button
                          variant="outline"
                          size="sm"
                          title="重新走 DNS → tcping → 测速，选出的 IP 不同则自动重启"
                          loading={
                            busyId === selected.id && busyAction === "reselect"
                          }
                          disabled={
                            nodeBusy || globalBusy || !selected.preferFastestIp
                          }
                          onClick={() => run(selected.id, "reselect")}
                        >
                          {!(
                            busyId === selected.id &&
                            busyAction === "reselect"
                          ) ? (
                            <RefreshCw className="size-3" strokeWidth={1.7} />
                          ) : null}
                          重选 IP
                        </Button>
                        <Button
                          variant="secondary"
                          size="sm"
                          className="text-muted-foreground"
                          loading={
                            busyId === selected.id && busyAction === "stop"
                          }
                          disabled={nodeBusy || globalBusy}
                          onClick={() => run(selected.id, "stop")}
                        >
                          {!(
                            busyId === selected.id && busyAction === "stop"
                          ) ? (
                            <Square className="size-3" strokeWidth={1.7} />
                          ) : null}
                          停止
                        </Button>
                      </>
                    ) : (
                      <Button
                        size="sm"
                        loading={
                          (busyId === selected.id &&
                            busyAction === "start") ||
                          selected.status === "starting"
                        }
                        disabled={nodeBusy || globalBusy}
                        onClick={() => run(selected.id, "start")}
                      >
                        {!(
                          busyId === selected.id && busyAction === "start"
                        ) ? (
                          <Play className="size-3" strokeWidth={1.7} />
                        ) : null}
                        启动
                      </Button>
                    )}
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label="编辑"
                      disabled={nodeBusy || globalBusy}
                      onClick={() => openEdit(selected)}
                    >
                      <Pencil className="size-3.5" strokeWidth={1.7} />
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon-sm"
                      aria-label="删除"
                      loading={
                        busyId === selected.id && busyAction === "remove"
                      }
                      disabled={nodeBusy || globalBusy}
                      onClick={() => run(selected.id, "remove")}
                    >
                      {!(
                        busyId === selected.id && busyAction === "remove"
                      ) ? (
                        <Trash2 className="size-3.5" strokeWidth={1.7} />
                      ) : null}
                    </Button>
                  </div>
                </div>

                {/* Endpoints strip */}
                <div className="mt-3 grid gap-2 sm:grid-cols-2">
                  <EndpointChip
                    label="本机 SOCKS"
                    value={`127.0.0.1:${selected.localPort}`}
                    mono={selected.socks}
                    onCopy={() => copyText(selected.socks, "本机 SOCKS 已复制")}
                  />
                  {selected.lanAccess ? (
                    <EndpointChip
                      label="局域网"
                      value={
                        selected.lanSocks
                          ? selected.lanSocks.replace("socks5://", "")
                          : `0.0.0.0:${selected.localPort}`
                      }
                      mono={selected.lanSocks || undefined}
                      onCopy={
                        selected.lanSocks
                          ? () =>
                              copyText(selected.lanSocks!, "局域网 SOCKS 已复制")
                          : undefined
                      }
                    />
                  ) : (
                    <div className="rounded-md bg-secondary/40 px-2.5 py-2 text-[11px] text-muted-foreground">
                      局域网接入未开启
                    </div>
                  )}
                </div>

                {/* Live status line */}
                <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
                  {selected.activeIp ? (
                    <span className="font-mono">
                      → {selected.activeIp}
                      {selected.lastProbe?.ms != null
                        ? ` · ${selected.lastProbe.ms}ms`
                        : ""}
                    </span>
                  ) : (
                    <span>上游 IP 未选定</span>
                  )}
                  {selected.lastTest?.ok ? (
                    <span className="text-success tabular-nums">
                      延迟 {selected.lastTest.ms}ms
                    </span>
                  ) : selected.lastTest && !selected.lastTest.ok ? (
                    <span className="text-destructive">
                      延迟失败 · {selected.lastTest.message}
                    </span>
                  ) : selected.status === "running" ? (
                    <span>点「延迟」测 simple RTT</span>
                  ) : null}
                  {selected.pid ? (
                    <span className="tabular-nums">pid {selected.pid}</span>
                  ) : null}
                  <span>
                    UDP {selected.udp ? "on" : "off"}
                    {" · "}
                    优选 {selected.preferFastestIp !== false ? "on" : "off"}
                  </span>
                </div>
              </div>

              {/* Logs fill remaining height */}
              <div className="flex min-h-0 flex-1 basis-0 flex-col">
                <div className="flex shrink-0 items-center justify-between gap-2 border-b border-border/50 px-4 py-1.5">
                  <div className="flex items-center gap-2 text-[11px] text-muted-foreground">
                    <Terminal className="size-3" strokeWidth={1.7} />
                    <span>运行日志</span>
                    <span className="tabular-nums opacity-70">
                      {logs.length}
                    </span>
                  </div>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-6 px-2"
                    onClick={async () => {
                      if (!selectedId) return;
                      try {
                        setLogs(await api.logs(selectedId));
                      } catch {
                        setLogs([]);
                      }
                    }}
                  >
                    <RefreshCw className="size-3" strokeWidth={1.7} />
                    刷新
                  </Button>
                </div>
                <div className="allow-select min-h-0 flex-1 basis-0 overflow-auto bg-card/40 px-4 py-2 font-mono text-[11px] leading-5 text-muted-foreground">
                  {logs.length === 0 ? (
                    <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
                      <p>暂无日志</p>
                      <p className="text-[10px] opacity-70">
                        启动后 DNS / tcping / 测速输出会出现在这里
                      </p>
                    </div>
                  ) : (
                    <>
                      {logs.map((log, idx) => (
                        <div
                          key={`${log.ts}-${idx}`}
                          className="whitespace-pre-wrap break-all"
                        >
                          <span className="text-foreground/35">
                            {new Date(log.ts).toLocaleTimeString()}
                          </span>{" "}
                          {log.line}
                        </div>
                      ))}
                      <div ref={logEndRef} />
                    </>
                  )}
                </div>
              </div>
            </>
          )}
        </section>
      </div>

      <Dialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        title={editing ? "编辑节点" : "添加节点"}
        description="上游 anytls 等协议 → 本地 SOCKS5。"
        busy={globalBusy && globalBusyLabel.includes("节点")}
        footer={
          <>
            <Button
              variant="secondary"
              disabled={globalBusy}
              onClick={() => setDialogOpen(false)}
            >
              取消
            </Button>
            <Button
              loading={globalBusy && globalBusyLabel.includes("节点")}
              onClick={saveNode}
            >
              保存
            </Button>
          </>
        }
      >
        <div className="grid gap-3 sm:grid-cols-2">
          <Field label="名称">
            <Input
              value={form.name}
              onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))}
              placeholder="例如 香港 01"
            />
          </Field>
          <Field label="协议类型">
            <Input
              value={form.type || "anytls"}
              onChange={(e) => setForm((f) => ({ ...f, type: e.target.value }))}
            />
          </Field>
          <Field label="服务器">
            <Input
              value={form.server}
              onChange={(e) => setForm((f) => ({ ...f, server: e.target.value }))}
              placeholder="host or ip"
            />
          </Field>
          <Field label="远程端口">
            <Input
              type="number"
              value={form.port}
              onChange={(e) =>
                setForm((f) => ({ ...f, port: Number(e.target.value) }))
              }
            />
          </Field>
          <Field label="密码">
            <Input
              value={form.password}
              onChange={(e) =>
                setForm((f) => ({ ...f, password: e.target.value }))
              }
            />
          </Field>
          <Field label="本地 SOCKS 端口">
            <Input
              type="number"
              value={form.localPort}
              onChange={(e) =>
                setForm((f) => ({ ...f, localPort: Number(e.target.value) }))
              }
            />
          </Field>
          <Field label="SNI">
            <Input
              value={form.sni || ""}
              onChange={(e) => setForm((f) => ({ ...f, sni: e.target.value }))}
            />
          </Field>
          <div className="flex flex-col justify-end gap-2 pb-1">
            <Checkbox
              id="udp"
              label="启用 UDP"
              checked={form.udp !== false}
              onCheckedChange={(v) => setForm((f) => ({ ...f, udp: v }))}
            />
            <Checkbox
              id="skip"
              label="跳过证书验证"
              checked={Boolean(form.skipCertVerify)}
              onCheckedChange={(v) =>
                setForm((f) => ({ ...f, skipCertVerify: v }))
              }
            />
            <Checkbox
              id="prefer"
              label="优选最快 IP（多地 DNS + tcping）"
              checked={form.preferFastestIp !== false}
              onCheckedChange={(v) =>
                setForm((f) => ({ ...f, preferFastestIp: v }))
              }
            />
            <Checkbox
              id="lan"
              label="允许局域网连入（0.0.0.0）"
              checked={Boolean(form.lanAccess)}
              onCheckedChange={(v) =>
                setForm((f) => ({ ...f, lanAccess: v }))
              }
            />
          </div>
        </div>
      </Dialog>

      <Dialog
        open={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        title="设置"
        description="iLatency 多地 DNS 与上游 IP 探活。"
        busy={settingsBusy}
        className="max-w-[560px]"
        footer={
          <>
            <Button
              variant="secondary"
              disabled={settingsBusy}
              onClick={() => setSettingsOpen(false)}
            >
              取消
            </Button>
            <Button
              variant="outline"
              loading={settingsBusy && settingsBusyLabel.includes("iLatency")}
              disabled={settingsBusy}
              onClick={testIlatency}
            >
              测试会话
            </Button>
            <Button
              loading={settingsBusy && settingsBusyLabel.includes("保存")}
              disabled={settingsBusy}
              onClick={saveSettings}
            >
              保存
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          <div className="rounded-md bg-secondary/40 p-3 text-[11px] leading-5 text-muted-foreground">
            浏览器登录{" "}
            <span className="font-mono text-foreground">ilatency.com</span>{" "}
            后复制 Cookie：
            <span className="font-mono text-foreground">
              {" "}
              nodeping_user_session
            </span>{" "}
            与{" "}
            <span className="font-mono text-foreground">sl-session</span>
            <div className="mt-1 font-mono text-[10px] text-foreground/80">
              nodeping_user_session=...; sl-session=...
            </div>
            {settings?.hasIlatencyCookie ? (
              <div className="mt-2 text-success">
                已配置
                {settings.ilatencyUser ? ` · ${settings.ilatencyUser}` : ""}
                {settings.ilatencyCookiePreview
                  ? ` · ${settings.ilatencyCookiePreview}`
                  : ""}
              </div>
            ) : (
              <div className="mt-2 text-destructive">
                未配置时仅本机 DNS + 公共 DoH，分运营商覆盖不全。
              </div>
            )}
          </div>
          <Field label="iLatency Cookie">
            <Input
              value={cookieDraft}
              onChange={(e) => setCookieDraft(e.target.value)}
              placeholder={
                settings?.hasIlatencyCookie
                  ? "留空则保留已保存 Cookie"
                  : "nodeping_user_session=...; sl-session=..."
              }
            />
          </Field>
          <div className="space-y-2 rounded-md bg-secondary/40 p-3">
            <div className="text-xs font-medium">启动行为</div>
            <Checkbox
              id="launch-at-login"
              label="开机自启"
              checked={launchAtLogin}
              onCheckedChange={setLaunchAtLogin}
              disabled={settingsBusy}
            />
            <Checkbox
              id="auto-start-nodes"
              label="启动应用时自动启动全部节点"
              checked={autoStartNodes}
              onCheckedChange={setAutoStartNodes}
              disabled={settingsBusy}
            />
            {settings?.launchAtLogin && !settings.launchAtLoginActive ? (
              <p className="text-[11px] text-amber-700 dark:text-amber-400">
                已勾选开机自启，但系统登录项尚未生效。保存后会重新注册。
              </p>
            ) : null}
          </div>
          <Field label="优选策略">
            <select
              className="flex h-8 w-full rounded-md border border-input bg-secondary/55 px-2.5 text-xs outline-none focus:bg-background focus:ring-1 focus:ring-ring"
              value={selectStrategy}
              onChange={(e) => setSelectStrategy(e.target.value)}
            >
              <option value="throughput">高带宽优先（推荐）</option>
              <option value="balanced">均衡（速度 / 延迟）</option>
              <option value="latency">低延迟优先</option>
            </select>
          </Field>
          <div className="grid gap-3 sm:grid-cols-2">
            <Field label="探活间隔（秒）">
              <Input
                type="number"
                min={3}
                max={120}
                value={healthInterval}
                onChange={(e) => setHealthInterval(Number(e.target.value) || 5)}
              />
            </Field>
            <Field label="切换冷却（秒）">
              <Input
                type="number"
                min={15}
                max={600}
                value={failoverCooldown}
                onChange={(e) =>
                  setFailoverCooldown(Number(e.target.value) || 60)
                }
              />
            </Field>
            <Field label="测速候选数 Top K">
              <Input
                type="number"
                min={1}
                max={8}
                value={speedTopK}
                onChange={(e) => setSpeedTopK(Number(e.target.value) || 3)}
              />
            </Field>
            <Field label="测速下载量（MB）">
              <Input
                type="number"
                min={0.25}
                max={8}
                step={0.25}
                value={speedTestMb}
                onChange={(e) => setSpeedTestMb(Number(e.target.value) || 1)}
              />
            </Field>
          </div>
          <p className="text-[11px] leading-5 text-muted-foreground">
            启动：多地 DNS → tcping → Top K 单线程测速（LibreSpeed，经 anytls 出口）→ 策略决选。运行中按间隔探活，失败后重选。
          </p>
        </div>
      </Dialog>
    </div>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <Label>{label}</Label>
      {children}
    </div>
  );
}

function EndpointChip({
  label,
  value,
  mono,
  onCopy,
}: {
  label: string;
  value: string;
  mono?: string;
  onCopy?: () => void;
}) {
  return (
    <div className="rounded-md bg-secondary/40 px-2.5 py-2">
      <div className="text-[10px] text-muted-foreground">{label}</div>
      {onCopy ? (
        <button
          type="button"
          className="mt-0.5 inline-flex max-w-full items-center gap-1.5 font-mono text-[12px] hover:underline"
          onClick={onCopy}
          title={mono || value}
        >
          <span className="truncate">{value}</span>
          <Copy className="size-3 shrink-0 opacity-50" strokeWidth={1.7} />
        </button>
      ) : (
        <div className="mt-0.5 truncate font-mono text-[12px]">{value}</div>
      )}
    </div>
  );
}
