import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppInfo,
  AppSettingsPatch,
  AppSettingsView,
  LogEntry,
  NodeInput,
  NodeTestResult,
  ProxyNode,
} from "./types";

export const api = {
  list: () => invoke<ProxyNode[]>("nodes_list"),
  create: (input: NodeInput) => invoke<ProxyNode[]>("nodes_create", { input }),
  update: (id: string, patch: Partial<NodeInput> & NodeInput) =>
    invoke<ProxyNode[]>("nodes_update", { id, patch }),
  remove: (id: string) => invoke<ProxyNode[]>("nodes_remove", { id }),
  start: (id: string) => invoke<ProxyNode[]>("nodes_start", { id }),
  stop: (id: string) => invoke<ProxyNode[]>("nodes_stop", { id }),
  startAll: () => invoke<ProxyNode[]>("nodes_start_all"),
  stopAll: () => invoke<ProxyNode[]>("nodes_stop_all"),
  test: (id: string) => invoke<NodeTestResult>("nodes_test", { id }),
  failover: (id: string) => invoke<ProxyNode[]>("nodes_failover", { id }),
  reselect: (id: string) => invoke<ProxyNode[]>("nodes_reselect", { id }),
  logs: (id: string) => invoke<LogEntry[]>("nodes_logs", { id }),
  getInfo: () => invoke<AppInfo>("app_get_info"),
  openPath: (path: string) => invoke<void>("app_open_path", { path }),
  getSettings: () => invoke<AppSettingsView>("settings_get"),
  updateSettings: (patch: AppSettingsPatch) =>
    invoke<AppSettingsView>("settings_update", { patch }),
  testIlatency: () => invoke<string>("settings_test_ilatency"),
  onChanged: (cb: (nodes: ProxyNode[]) => void): (() => void) => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    void listen<ProxyNode[]>("nodes:changed", (e) => cb(e.payload))
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch((error) => console.error("Failed to subscribe to node changes:", error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  },
  onLog: (
    cb: (payload: { id: string; ts: string; line: string }) => void,
  ): (() => void) => {
    let unlisten: UnlistenFn | undefined;
    let disposed = false;
    void listen<{ id: string; ts: string; line: string }>(
      "nodes:log",
      (e) => cb(e.payload),
    )
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch((error) => console.error("Failed to subscribe to node logs:", error));
    return () => {
      disposed = true;
      unlisten?.();
    };
  },
};
