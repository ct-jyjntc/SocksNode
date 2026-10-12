export type NodeStatus =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "error";

export type NodeTestResult = {
  ok: boolean;
  ms: number;
  ip: string | null;
  message: string;
  at: string;
};

export type ProbeStatus = {
  ok: boolean;
  ms: number | null;
  ip: string | null;
  message: string;
  at: string;
};

export type ProxyNode = {
  id: string;
  name: string;
  type: string;
  server: string;
  port: number;
  password: string;
  udp: boolean;
  sni: string;
  skipCertVerify: boolean;
  sessionReuse: boolean;
  localPort: number;
  enabled: boolean;
  preferFastestIp: boolean;
  lanAccess: boolean;
  activeIp?: string | null;
  lastProbe?: ProbeStatus | null;
  status: NodeStatus;
  pid: number | null;
  lastError: string | null;
  lastTest?: NodeTestResult | null;
  updatedAt?: string | null;
  socks: string;
  lanSocks?: string | null;
  running: boolean;
};

export type NodeInput = {
  name: string;
  type?: string;
  server: string;
  port: number;
  password: string;
  udp?: boolean;
  sni?: string;
  skipCertVerify?: boolean;
  sessionReuse?: boolean;
  localPort: number;
  enabled?: boolean;
  preferFastestIp?: boolean;
  lanAccess?: boolean;
};

export type AppInfo = {
  dataDir: string;
  kernelPath: string;
  version: string;
  /** Config problems found at startup (corrupt files, disabled duplicates). */
  startupWarnings: string[];
};

export type SelectStrategy = "balanced" | "latency" | "throughput";

export type AppSettingsView = {
  hasIlatencyCookie: boolean;
  ilatencyCookiePreview: string;
  healthIntervalSecs: number;
  failoverCooldownSecs: number;
  ilatencyUser: string | null;
  selectStrategy: SelectStrategy | string;
  speedTopK: number;
  speedTestBytes: number;
  speedTestTimeoutSecs: number;
  latencyTimeoutSecs: number;
  launchAtLogin: boolean;
  startMinimized: boolean;
  autoStartNodes: boolean;
  launchAtLoginActive: boolean;
};

export type AppSettingsPatch = {
  ilatencyCookie?: string;
  healthIntervalSecs?: number;
  failoverCooldownSecs?: number;
  selectStrategy?: string;
  speedTopK?: number;
  speedTestBytes?: number;
  speedTestTimeoutSecs?: number;
  latencyTimeoutSecs?: number;
  launchAtLogin?: boolean;
  startMinimized?: boolean;
  autoStartNodes?: boolean;
};

export type LogEntry = {
  ts: string;
  line: string;
};
