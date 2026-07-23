import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  Activity,
  Cable,
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
} from "lucide-react";
import { Toaster, toast } from "sonner";
import { Badge, Button, Card, Checkbox, Dialog, Input, Label } from "@/components/ui";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "@/lib/api";
import type { AppInfo, LogEntry, NodeInput, ProxyNode } from "@/lib/types";
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

export default function App() {
  const { dark, setDark } = useTheme();
  const [nodes, setNodes] = useState<ProxyNode[]>([]);
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [globalBusy, setGlobalBusy] = useState(false);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editing, setEditing] = useState<ProxyNode | null>(null);
  const [form, setForm] = useState<NodeInput>(emptyForm);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [query, setQuery] = useState("");

  const refresh = useCallback(async () => {
    const list = await api.list();
    setNodes(list);
    setLoading(false);
  }, []);

  useEffect(() => {
    let unsubChanged = () => {};
    let unsubLog = () => {};

    (async () => {
      try {
        const [list, appInfo] = await Promise.all([
          api.list(),
          api.getInfo(),
        ]);
        setNodes(list);
        setInfo(appInfo);
        if (list[0]) setSelectedId(list[0].id);
      } catch (err) {
        toast.error(err instanceof Error ? err.message : "初始化失败");
      } finally {
        setLoading(false);
      }
    })();

    unsubChanged = api.onChanged((list) => {
      setNodes(list);
    });

    unsubLog = api.onLog((payload) => {
      setSelectedId((current) => {
        if (current === payload.id) {
          setLogs((prev) => {
            const next = [...prev, { ts: payload.ts, line: payload.line }];
            return next.slice(-400);
          });
        }
        return current;
      });
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
    });
    setDialogOpen(true);
  };

  const saveNode = async () => {
    try {
      setGlobalBusy(true);
      if (editing) {
        await api.update(editing.id, form);
        toast.success("节点已更新");
      } else {
        await api.create(form);
        toast.success("节点已添加");
      }
      setDialogOpen(false);
      await refresh();
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "保存失败");
    } finally {
      setGlobalBusy(false);
    }
  };

  const run = async (id: string, action: "start" | "stop" | "remove" | "test") => {
    try {
      setBusyId(id);
      if (action === "start") {
        await api.start(id);
        toast.success("本地 SOCKS 已启动（不等于上游一定可用）");
      } else if (action === "stop") {
        await api.stop(id);
        toast.success("节点已停止");
      } else if (action === "test") {
        const result = await api.test(id);
        if (result.ok) {
          toast.success(`${result.message}`);
        } else {
          toast.error(result.message);
        }
        await refresh();
      } else {
        await api.remove(id);
        toast.success("节点已删除");
        if (selectedId === id) setSelectedId(null);
      }
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "操作失败");
      await refresh();
    } finally {
      setBusyId(null);
    }
  };

  const startAll = async () => {
    try {
      setGlobalBusy(true);
      await api.startAll();
      toast.success("全部节点已启动");
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "批量启动失败");
    } finally {
      setGlobalBusy(false);
    }
  };

  const stopAll = async () => {
    try {
      setGlobalBusy(true);
      await api.stopAll();
      toast.success("全部节点已停止");
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "批量停止失败");
    } finally {
      setGlobalBusy(false);
    }
  };

  const copyText = async (text: string, label = "已复制") => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(label);
    } catch {
      toast.error("复制失败");
    }
  };

  return (
    <div className="min-h-full bg-background">
      <Toaster richColors closeButton position="top-right" />

      {/* Titlebar drag region: data-tauri-drag-region + explicit startDragging */}
      <div
        data-tauri-drag-region
        className="app-drag fixed inset-x-0 top-0 z-50 h-12 select-none"
        onMouseDown={(e) => {
          // left button only; skip interactive targets
          if (e.button !== 0) return;
          const el = e.target as HTMLElement;
          if (el.closest("button, a, input, textarea, select, [role=button]")) {
            return;
          }
          e.preventDefault();
          void getCurrentWindow().startDragging();
        }}
      />

      <div className="mx-auto flex min-h-full w-full max-w-[1280px] flex-col px-5 pb-8 pt-14 sm:px-8 lg:pb-14 lg:pt-16">
        <header className="app-no-drag mb-8 flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="mb-2 inline-flex items-center gap-2 text-xs text-muted-foreground">
              <Cable className="size-3.5" strokeWidth={1.7} />
              SocksNode · anytls local bridge
            </div>
            <h1 className="text-2xl font-medium tracking-tight">节点转发</h1>
            <p className="mt-1 max-w-xl text-xs leading-5 text-muted-foreground">
              每个节点独立启动一个 anytls-client 实例，仅暴露本地 SOCKS。不启用系统代理与虚拟网卡。
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="ghost"
              size="icon"
              aria-label="切换主题"
              onClick={() => setDark(!dark)}
            >
              {dark ? (
                <Sun className="size-4" strokeWidth={1.7} />
              ) : (
                <Moon className="size-4" strokeWidth={1.7} />
              )}
            </Button>
            {info ? (
              <Button
                variant="outline"
                onClick={() => api.openPath(info.dataDir)}
              >
                <FolderOpen className="size-3.5" strokeWidth={1.7} />
                数据目录
              </Button>
            ) : null}
            <Button variant="secondary" disabled={globalBusy} onClick={stopAll}>
              <Square className="size-3.5" strokeWidth={1.7} />
              全部停止
            </Button>
            <Button disabled={globalBusy} onClick={startAll}>
              <Play className="size-3.5" strokeWidth={1.7} />
              全部启动
            </Button>
            <Button onClick={openCreate}>
              <Plus className="size-3.5" strokeWidth={1.7} />
              添加节点
            </Button>
          </div>
        </header>

        <section className="mb-6 grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
          <Metric title="节点总数" value={stats.total} />
          <Metric title="运行中" value={stats.running} accent="success" />
          <Metric title="已停止" value={stats.stopped} />
          <Metric title="异常" value={stats.errored} accent="danger" />
        </section>

        <div className="app-no-drag grid gap-4 xl:grid-cols-[1.4fr_0.9fr]">
          <Card className="min-h-[420px] p-0">
            <div className="flex flex-wrap items-center justify-between gap-3 border-b border-border/70 px-4 py-3">
              <div>
                <h2 className="text-sm font-medium">节点列表</h2>
                <p className="mt-0.5 text-xs text-muted-foreground">
                  本地 SOCKS · 一节点一进程
                </p>
              </div>
              <div className="flex items-center gap-2">
                <Input
                  className="w-48"
                  placeholder="搜索名称 / 地址 / 端口"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                />
                <Button variant="ghost" size="icon" onClick={refresh} aria-label="刷新">
                  <RefreshCw className="size-3.5" strokeWidth={1.7} />
                </Button>
              </div>
            </div>

            <div className="overflow-x-auto">
              <table className="w-full min-w-[720px] text-left">
                <thead>
                  <tr className="h-9 border-b border-border/60 text-xs font-normal text-muted-foreground">
                    <th className="px-4 font-normal">节点</th>
                    <th className="px-2 font-normal">上游</th>
                    <th className="px-2 font-normal">本地 SOCKS</th>
                    <th className="px-2 font-normal">状态</th>
                    <th className="px-4 text-right font-normal">操作</th>
                  </tr>
                </thead>
                <tbody>
                  {loading ? (
                    <tr>
                      <td colSpan={5} className="px-4 py-16 text-center text-xs text-muted-foreground">
                        加载中…
                      </td>
                    </tr>
                  ) : filtered.length === 0 ? (
                    <tr>
                      <td colSpan={5} className="px-4 py-16 text-center text-xs text-muted-foreground">
                        暂无节点
                      </td>
                    </tr>
                  ) : (
                    filtered.map((node) => {
                      const active = selectedId === node.id;
                      return (
                        <tr
                          key={node.id}
                          className={cn(
                            "cursor-pointer border-b border-border/40 text-xs transition-colors hover:bg-secondary/45",
                            active && "bg-secondary/60",
                          )}
                          onClick={() => setSelectedId(node.id)}
                        >
                          <td className="px-4 py-3 align-top">
                            <div className="font-medium text-foreground">{node.name}</div>
                            <div className="mt-0.5 font-mono text-[11px] text-muted-foreground">
                              {node.type}
                            </div>
                          </td>
                          <td className="px-2 py-3 align-top">
                            <div className="font-mono text-[11px]">
                              {node.server}:{node.port}
                            </div>
                            <div className="mt-0.5 text-[11px] text-muted-foreground">
                              sni {node.sni || "—"}
                            </div>
                          </td>
                          <td className="px-2 py-3 align-top">
                            <button
                              type="button"
                              className="inline-flex items-center gap-1 font-mono text-[11px] text-foreground hover:underline"
                              onClick={(e) => {
                                e.stopPropagation();
                                copyText(node.socks, "SOCKS 地址已复制");
                              }}
                            >
                              127.0.0.1:{node.localPort}
                              <Copy className="size-3 opacity-60" strokeWidth={1.7} />
                            </button>
                            {node.pid ? (
                              <div className="mt-0.5 text-[11px] text-muted-foreground">
                                pid {node.pid}
                              </div>
                            ) : null}
                          </td>
                          <td className="px-2 py-3 align-top">
                            <Badge tone={statusTone(node.status)}>
                              {statusLabel(node.status)}
                            </Badge>
                            {node.lastTest?.ok ? (
                              <div className="mt-1 max-w-[200px] truncate text-[11px] text-success">
                                出口 {node.lastTest.ip} · {node.lastTest.ms}ms
                              </div>
                            ) : null}
                            {node.lastError ? (
                              <div className="mt-1 max-w-[180px] truncate text-[11px] text-destructive">
                                {node.lastError}
                              </div>
                            ) : null}
                          </td>
                          <td className="px-4 py-3 align-top">
                            <div
                              className="flex items-center justify-end gap-1"
                              onClick={(e) => e.stopPropagation()}
                            >
                              {node.status === "running" ? (
                                <>
                                  <Button
                                    variant="outline"
                                    size="sm"
                                    disabled={busyId === node.id}
                                    onClick={() => run(node.id, "test")}
                                  >
                                    <Activity className="size-3" strokeWidth={1.7} />
                                    测试
                                  </Button>
                                  <Button
                                    variant="secondary"
                                    size="sm"
                                    disabled={busyId === node.id}
                                    onClick={() => run(node.id, "stop")}
                                  >
                                    <Square className="size-3" strokeWidth={1.7} />
                                    停止
                                  </Button>
                                </>
                              ) : (
                                <Button
                                  size="sm"
                                  disabled={busyId === node.id}
                                  onClick={() => run(node.id, "start")}
                                >
                                  <Play className="size-3" strokeWidth={1.7} />
                                  启动
                                </Button>
                              )}
                              <Button
                                variant="ghost"
                                size="icon"
                                aria-label="编辑"
                                onClick={() => openEdit(node)}
                              >
                                <Pencil className="size-3.5" strokeWidth={1.7} />
                              </Button>
                              <Button
                                variant="ghost"
                                size="icon"
                                aria-label="删除"
                                onClick={() => run(node.id, "remove")}
                              >
                                <Trash2 className="size-3.5" strokeWidth={1.7} />
                              </Button>
                            </div>
                          </td>
                        </tr>
                      );
                    })
                  )}
                </tbody>
              </table>
            </div>
          </Card>

          <div className="space-y-4">
            <Card>
              <div className="mb-3 flex items-center justify-between">
                <h2 className="text-sm font-medium">选中节点</h2>
                <Terminal className="size-3.5 text-muted-foreground" strokeWidth={1.7} />
              </div>
              {selectedNode(nodes, selectedId) ? (
                <SelectedDetail
                  node={selectedNode(nodes, selectedId)!}
                  onCopy={copyText}
                />
              ) : (
                <p className="text-xs text-muted-foreground">选择左侧节点查看详情与日志。</p>
              )}
            </Card>

            <Card className="min-h-[280px]">
              <div className="mb-3 flex items-center justify-between">
                <h2 className="text-sm font-medium">运行日志</h2>
                <span className="text-[11px] text-muted-foreground">{logs.length} lines</span>
              </div>
              <div className="h-[280px] overflow-auto rounded-md bg-background/70 p-3 font-mono text-[11px] leading-5 text-muted-foreground">
                {logs.length === 0 ? (
                  <div className="flex h-full items-center justify-center">暂无日志</div>
                ) : (
                  logs.map((log, idx) => (
                    <div key={`${log.ts}-${idx}`} className="whitespace-pre-wrap break-all">
                      <span className="text-foreground/40">
                        {new Date(log.ts).toLocaleTimeString()}
                      </span>{" "}
                      {log.line}
                    </div>
                  ))
                )}
              </div>
            </Card>

            {info ? (
              <Card>
                <h2 className="mb-3 text-sm font-medium">运行环境</h2>
                <dl className="space-y-2 text-xs">
                  <Row label="版本" value={info.version} />
                  <Row label="内核" value={info.mihomoPath} mono />
                  <Row label="数据" value={info.dataDir} mono />
                </dl>
              </Card>
            ) : null}
          </div>
        </div>

        <footer className="mt-10 text-[11px] text-muted-foreground">
          Quiet Console · local only · no system proxy / no TUN
        </footer>
      </div>

      <Dialog
        open={dialogOpen}
        onClose={() => setDialogOpen(false)}
        title={editing ? "编辑节点" : "添加节点"}
        description="将上游 anytls 等协议节点转发为本地 SOCKS5。"
        footer={
          <>
            <Button variant="secondary" onClick={() => setDialogOpen(false)}>
              取消
            </Button>
            <Button disabled={globalBusy} onClick={saveNode}>
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
              onChange={(e) => setForm((f) => ({ ...f, password: e.target.value }))}
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
          </div>
        </div>
      </Dialog>
    </div>
  );
}

function selectedNode(nodes: ProxyNode[], id: string | null) {
  return nodes.find((n) => n.id === id) || null;
}

function Metric({
  title,
  value,
  accent,
}: {
  title: string;
  value: number;
  accent?: "success" | "danger";
}) {
  return (
    <Card className="min-h-24">
      <div className="text-xs text-muted-foreground">{title}</div>
      <div
        className={cn(
          "mt-2 text-xl font-medium tabular-nums",
          accent === "success" && "text-success",
          accent === "danger" && "text-destructive",
        )}
      >
        {value}
      </div>
    </Card>
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
    <div className="space-y-2">
      <Label>{label}</Label>
      {children}
    </div>
  );
}

function Row({
  label,
  value,
  mono,
}: {
  label: string;
  value: string;
  mono?: boolean;
}) {
  return (
    <div className="grid grid-cols-[72px_1fr] gap-2">
      <dt className="text-muted-foreground">{label}</dt>
      <dd className={cn("break-all", mono && "font-mono text-[11px]")}>{value}</dd>
    </div>
  );
}

function SelectedDetail({
  node,
  onCopy,
}: {
  node: ProxyNode;
  onCopy: (text: string, label?: string) => void;
}) {
  return (
    <div className="space-y-3 text-xs">
      <div>
        <div className="font-medium">{node.name}</div>
        <div className="mt-1 text-muted-foreground">
          {node.server}:{node.port} · {node.type}
        </div>
      </div>
      <div className="rounded-md bg-background/70 p-3">
        <div className="mb-1 text-[11px] text-muted-foreground">本地 SOCKS5</div>
        <button
          type="button"
          className="inline-flex items-center gap-1.5 font-mono text-[12px] hover:underline"
          onClick={() => onCopy(node.socks, "SOCKS 地址已复制")}
        >
          {node.socks}
          <Copy className="size-3 opacity-60" strokeWidth={1.7} />
        </button>
      </div>
      {node.lastTest ? (
        <div
          className={cn(
            "rounded-md p-3 text-[11px]",
            node.lastTest.ok
              ? "bg-success/10 text-success"
              : "bg-destructive/10 text-destructive",
          )}
        >
          {node.lastTest.ok
            ? `连通正常 · 出口 ${node.lastTest.ip} · ${node.lastTest.ms}ms`
            : `连通失败 · ${node.lastTest.message}`}
        </div>
      ) : (
        <div className="rounded-md bg-background/70 p-3 text-[11px] text-muted-foreground">
          「运行中」仅表示本地 SOCKS 端口已监听。请点「测试」验证上游是否真正可用。
        </div>
      )}
      <dl className="space-y-1.5 text-[11px] text-muted-foreground">
        <div>SNI · {node.sni || "—"}</div>
        <div>UDP · {node.udp ? "on" : "off"}</div>
        <div>skip-cert-verify · {node.skipCertVerify ? "true" : "false"}</div>
        <div>status · {statusLabel(node.status)}</div>
      </dl>
    </div>
  );
}
