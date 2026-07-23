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
  localPort: number;
  enabled: boolean;
  status: NodeStatus;
  pid: number | null;
  lastError: string | null;
  lastTest?: NodeTestResult | null;
  updatedAt?: string;
  socks: string;
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
  localPort: number;
  enabled?: boolean;
};

export type AppInfo = {
  dataDir: string;
  mihomoPath: string;
  version: string;
};

export type LogEntry = {
  ts: string;
  line: string;
};
