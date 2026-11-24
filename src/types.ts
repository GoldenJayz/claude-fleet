// Mirrors crates/collector/src/types.rs (API v1). Times are Unix milliseconds.

export interface MachineInfo {
  apiVersion: number;
  collectorVersion: string;
  machineName: string;
  hostname: string;
  os: string;
  cpuBrand: string;
  logicalCores: number;
  physicalCores: number | null;
  totalMemory: number;
  bootTimeMs: number;
  port: number;
  /** Optional API features, e.g. 'files'. Missing on older collectors. */
  features?: string[];
}

export interface DiskInfo {
  name: string;
  mount: string;
  kind: string;
  total: number;
  available: number;
  readBps: number;
  writeBps: number;
}

export interface GpuInfo {
  name: string;
  vendor: string | null;
  usage: number | null;
  memoryUsed: number | null;
  memoryTotal: number | null;
  temperatureC: number | null;
}

export interface TempReading {
  label: string;
  celsius: number;
}

export interface ProcessInfo {
  pid: number;
  parentPid: number | null;
  name: string;
  exe: string | null;
  cpu: number;
  memory: number;
  runTimeS: number;
  startTime: number;
}

export interface HeavyProcess {
  pid: number;
  name: string;
  exe: string | null;
  startTime: number;
  cpuAvg: number;
  memory: number;
  since: number;
  reason: 'cpu' | 'memory' | 'cpu+memory';
  protectedReason: string | null;
}

export interface HeavyRules {
  cpuPct: number;
  memoryBytes: number;
  sustainMs: number;
}

export interface KillImpact {
  pid: number;
  name: string;
  exe: string | null;
  startTime: number;
  cpu: number;
  memory: number;
  protectedReason: string | null;
  treeCount: number;
  treeCpu: number;
  treeMemory: number;
  treeProtected: string[];
}

export interface KillOutcome {
  pid: number;
  name: string;
  memory: number;
  cpu: number;
  ok: boolean;
  needsAdmin: boolean;
  error: string | null;
}

export interface KillResult {
  ok: boolean;
  tree: boolean;
  outcomes: KillOutcome[];
  skippedProtected: string[];
}

export interface KillLogEntry {
  id: number;
  ts: number;
  machine: string;
  pid: number;
  name: string;
  tree: boolean;
  requestedBy: string;
  ok: boolean;
  detail: string | null;
}

export interface MetricsSample {
  ts: number;
  cpu: number;
  cpuPerCore: number[];
  cpuFreqMhz: number;
  memUsed: number;
  memTotal: number;
  swapUsed: number;
  swapTotal: number;
  disks: DiskInfo[];
  diskReadBps: number;
  diskWriteBps: number;
  netRxBps: number;
  netTxBps: number;
  gpus: GpuInfo[];
  temps: TempReading[];
}

export interface MetricsPoint {
  ts: number;
  cpu: number;
  memPct: number;
  gpu: number | null;
  diskReadBps: number;
  diskWriteBps: number;
  netRxBps: number;
  netTxBps: number;
}

export interface TokenTotals {
  input: number;
  output: number;
  cacheWrite: number;
  cacheRead: number;
  thinking: number;
  requests: number;
  prompts: number;
}

export interface ModelTotals {
  model: string;
  tokens: TokenTotals;
}

export interface Activity {
  ts: number;
  kind: 'thinking' | 'text' | 'tool';
  tool: string | null;
  detail: string | null;
  inSubagent: boolean;
}

export type PromptStatus = 'working' | 'done' | 'stopped';

export interface PromptEntry {
  id: string;
  sessionId: string;
  ts: number;
  kind: 'prompt' | 'command' | 'bash';
  text: string;
  project: string | null;
  model: string | null;
  effort: string | null;
  effortIsDefault: boolean;
  tokens: TokenTotals;
  subagentTokens: number;
  active: boolean;
  status: PromptStatus;
  activity: Activity | null;
  lastActivityTs: number | null;
}

export interface ClaudeSummary {
  today: TokenTotals;
  week: TokenTotals;
  todayByModel: ModelTotals[];
  recent: PromptEntry[];
  working: PromptEntry[];
  defaultEffort: string | null;
  lastActivityTs: number | null;
  filesTracked: number;
}

export interface LimitWindow {
  usedPercentage: number;
  resetsAtMs: number;
}

export interface PlanReading {
  capturedAtMs: number;
  fiveHour: LimitWindow | null;
  sevenDay: LimitWindow | null;
  claudeVersion: string | null;
  /** 'anthropic' (asked Anthropic directly) or 'statusLine'. Missing on older collectors. */
  source?: 'anthropic' | 'statusLine';
}

export interface StatusLineState {
  installed: boolean;
  chainedCommand: string | null;
}

export interface Snapshot {
  machine: MachineInfo;
  metrics: MetricsSample | null;
  history: MetricsPoint[];
  processes: ProcessInfo[];
  heavy: HeavyProcess[];
  heavyRules: HeavyRules;
  claude: ClaudeSummary;
  plan: PlanReading | null;
  statusLine: StatusLineState;
  /** Recent agent tasks on this machine (without steps). Missing on older collectors. */
  tasks?: AgentTask[];
}

export type AgentMode = 'plan' | 'acceptEdits' | 'auto' | 'bypassPermissions';
export type AgentTaskStatus = 'queued' | 'running' | 'done' | 'failed' | 'cancelled';

export interface AgentStep {
  ts: number;
  kind: 'note' | 'text' | 'tool' | 'toolError' | 'error';
  text: string;
}

/** A job handed to Claude Code on a machine. */
export interface AgentTask {
  id: number;
  created: number;
  started: number | null;
  ended: number | null;
  from: string;
  ip: string;
  prompt: string;
  cwd: string;
  mode: AgentMode;
  model: string | null;
  effort: string | null;
  resume: string | null;
  followUpOf?: number | null;
  status: AgentTaskStatus;
  sessionId: string | null;
  result: string | null;
  error: string | null;
  costUsd: number | null;
  turns: number | null;
  stepCount: number;
  steps: AgentStep[];
}

export interface AgentStatus {
  access: 'off' | 'edits' | 'full';
  machine: string;
  running: number | null;
  queued: number;
  folders: string[];
  home: string | null;
}

export type StreamEvent =
  | ({ type: 'snapshot' } & Snapshot)
  | { type: 'metrics'; sample: MetricsSample; processes: ProcessInfo[]; heavy: HeavyProcess[] }
  | ({ type: 'claude' } & ClaudeSummary)
  | ({ type: 'plan' } & PlanReading)
  | ({ type: 'fileReceived' } & ReceivedFile)
  | ({ type: 'shell' } & ShellSession)
  | ({ type: 'task' } & AgentTask)
  | { type: 'screenViewed'; ts: number; from: string; ip: string }
  | { type: 'taskStep'; taskId: number; step: AgentStep };

/** A file another PC sent to this one. */
export interface ReceivedFile {
  id: number;
  ts: number;
  name: string;
  sentName: string;
  size: number;
  from: string;
  ip: string;
  path: string;
}

export interface ShellSession {
  id: number;
  started: number;
  ended: number | null;
  from: string;
  ip: string;
  shell: string;
}

export interface ShellStatus {
  enabled: boolean;
  shell: string;
  active: number;
  machine: string;
}

/** One entry in the dashboard's machine list. */
export interface MachineConfig {
  id: string;
  name: string;
  /** host:port */
  address: string;
  token: string;
  isLocal?: boolean;
}

export type ConnectionStatus = 'connecting' | 'online' | 'offline' | 'unauthorized';
