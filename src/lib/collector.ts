import { useEffect, useRef, useState } from 'react';
import type {
  ConnectionStatus,
  KillImpact,
  KillLogEntry,
  KillResult,
  MachineConfig,
  MetricsPoint,
  ReceivedFile,
  ShellStatus,
  Snapshot,
  StreamEvent,
  TokenTotals,
} from '../types';
import { isVisible, onVisibility } from './visibility';

const SPARK_POINTS = 30;
/** No message for this long → offline (samples arrive every 2 s). */
const STALE_MS = 9000;

export interface CollectorState {
  status: ConnectionStatus;
  snapshot: Snapshot | null;
  lastMessageAt: number | null;
  error: string | null;
}

export function httpBase(m: MachineConfig) {
  return `http://${m.address}/v1`;
}

export async function apiGet<T>(m: MachineConfig, path: string): Promise<T> {
  const r = await fetch(`${httpBase(m)}${path}`, { headers: { Authorization: `Bearer ${m.token}` } });
  if (!r.ok) throw Object.assign(new Error(`${r.status}`), { status: r.status });
  return r.json() as Promise<T>;
}

export function tokensBetween(m: MachineConfig, from: number, to?: number) {
  return apiGet<TokenTotals>(m, `/claude/tokens?from=${Math.floor(from)}${to ? `&to=${Math.floor(to)}` : ''}`);
}

async function errorText(r: Response) {
  try {
    return ((await r.json()) as { error?: string }).error ?? `HTTP ${r.status}`;
  } catch {
    return `HTTP ${r.status}`;
  }
}

export async function killImpact(m: MachineConfig, pid: number, startTime: number): Promise<KillImpact> {
  const r = await fetch(`${httpBase(m)}/processes/${pid}/impact?startTime=${startTime}`, { headers: { Authorization: `Bearer ${m.token}` } });
  if (!r.ok) throw new Error(await errorText(r));
  return r.json();
}

export async function killProcess(m: MachineConfig, pid: number, startTime: number, tree: boolean, requestedBy: string): Promise<KillResult> {
  const r = await fetch(`${httpBase(m)}/processes/${pid}/kill`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${m.token}`, 'Content-Type': 'application/json' },
    body: JSON.stringify({ tree, startTime, requestedBy }),
  });
  if (!r.ok) throw new Error(await errorText(r));
  return r.json();
}

export function killLog(m: MachineConfig, limit = 30) {
  return apiGet<KillLogEntry[]>(m, `/kills?limit=${limit}`);
}

export async function shellStatus(m: MachineConfig): Promise<ShellStatus> {
  const r = await fetch(`${httpBase(m)}/shell/status`, { headers: { Authorization: `Bearer ${m.token}` } });
  if (r.status === 404) throw new Error('That PC runs an older Claude Fleet without remote shell. Update it first.');
  if (!r.ok) throw new Error(await errorText(r));
  return r.json();
}

export function shellUrl(m: MachineConfig, cols: number, rows: number, from: string) {
  return `ws://${m.address}/v1/shell?token=${encodeURIComponent(m.token)}&cols=${cols}&rows=${rows}&from=${encodeURIComponent(from)}`;
}

export interface Upload {
  promise: Promise<ReceivedFile>;
  abort: () => void;
}

/** Stream one file to a machine's collector, reporting progress. */
export function uploadFile(m: MachineConfig, file: File, from: string, onProgress: (sent: number) => void): Upload {
  const xhr = new XMLHttpRequest();
  const promise = new Promise<ReceivedFile>((resolve, reject) => {
    xhr.open('PUT', `${httpBase(m)}/files?name=${encodeURIComponent(file.name)}&from=${encodeURIComponent(from)}`);
    xhr.setRequestHeader('Authorization', `Bearer ${m.token}`);
    xhr.setRequestHeader('Content-Type', 'application/octet-stream');
    xhr.upload.onprogress = (e) => onProgress(e.loaded);
    xhr.onload = () => {
      let body: { error?: string } & Partial<ReceivedFile> = {};
      try {
        body = JSON.parse(xhr.responseText);
      } catch {
        /* not JSON */
      }
      if (xhr.status === 200) resolve(body as ReceivedFile);
      else if (xhr.status === 404) reject(new Error('That PC runs an older Claude Fleet that can’t receive files. Update it first.'));
      else reject(new Error(body.error ?? `HTTP ${xhr.status}`));
    };
    xhr.onerror = () => reject(new Error(`Couldn’t reach ${m.name}. The connection dropped.`));
    xhr.onabort = () => reject(Object.assign(new Error('Cancelled'), { cancelled: true }));
    xhr.send(file);
  });
  return { promise, abort: () => xhr.abort() };
}

function apply(prev: Snapshot | null, ev: StreamEvent): Snapshot | null {
  switch (ev.type) {
    case 'snapshot': {
      const { type: _t, ...snap } = ev;
      return snap;
    }
    case 'metrics': {
      if (!prev) return prev;
      const s = ev.sample;
      const gpu = [...s.gpus].sort((a, b) => (b.memoryTotal ?? 0) - (a.memoryTotal ?? 0))[0];
      const point: MetricsPoint = {
        ts: s.ts,
        cpu: s.cpu,
        memPct: s.memTotal ? (s.memUsed * 100) / s.memTotal : 0,
        gpu: gpu?.usage ?? null,
        diskReadBps: s.diskReadBps,
        diskWriteBps: s.diskWriteBps,
        netRxBps: s.netRxBps,
        netTxBps: s.netTxBps,
      };
      return { ...prev, metrics: s, processes: ev.processes, heavy: ev.heavy, history: [...prev.history, point].slice(-SPARK_POINTS) };
    }
    case 'claude': {
      if (!prev) return prev;
      const { type: _t, ...claude } = ev;
      return { ...prev, claude };
    }
    case 'plan': {
      if (!prev) return prev;
      const { type: _t, ...plan } = ev;
      return { ...prev, plan };
    }
    default:
      return prev;
  }
}

export type CollectorEvent = Extract<StreamEvent, { type: 'fileReceived' } | { type: 'shell' }>;

/** Live connection to one collector: WebSocket stream with reconnect and offline detection. */
export function useCollector(machine: MachineConfig, onEvent?: (ev: CollectorEvent) => void): CollectorState {
  const [state, setState] = useState<CollectorState>({ status: 'connecting', snapshot: null, lastMessageAt: null, error: null });
  const lastMsg = useRef(0);
  const onEventRef = useRef(onEvent);
  onEventRef.current = onEvent;

  useEffect(() => {
    let ws: WebSocket | null = null;
    let closed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let backoff = 1000;
    // Latest data, kept even while the window is hidden; React only re-renders when it's visible.
    let latest: Snapshot | null = null;
    let pending = false;

    const markOffline = (error: string | null, status: ConnectionStatus = 'offline') =>
      setState((s) => ({ ...s, status, error }));

    async function connect() {
      if (closed) return;
      // Probe over HTTP first: a WebSocket can't tell us *why* it failed (wrong token vs. down).
      try {
        await apiGet(machine, '/info');
      } catch (e) {
        const status = (e as { status?: number }).status;
        if (status === 401) markOffline('The pairing token was rejected.', 'unauthorized');
        else if (status === 403) markOffline('The collector only accepts LAN addresses.');
        else markOffline(`Can't reach ${machine.address}.`);
        schedule();
        return;
      }
      ws = new WebSocket(`ws://${machine.address}/v1/stream?token=${encodeURIComponent(machine.token)}`);
      ws.onmessage = (msg) => {
        lastMsg.current = Date.now();
        backoff = 1000;
        const ev = JSON.parse(msg.data) as StreamEvent;
        if (ev.type === 'fileReceived' || ev.type === 'shell') {
          onEventRef.current?.(ev);
          return;
        }
        latest = apply(latest, ev);
        // Metrics arrive every 2 s; skip redrawing them while hidden. Plan readings always go
        // through so the 80% alert still fires from the tray.
        if (ev.type === 'metrics' && !isVisible()) {
          pending = true;
          return;
        }
        pending = false;
        const snap = latest;
        setState({ status: 'online', error: null, lastMessageAt: lastMsg.current, snapshot: snap });
      };
      ws.onclose = () => {
        ws = null;
        if (!closed) {
          markOffline(`Lost connection to ${machine.address}.`);
          schedule();
        }
      };
      ws.onerror = () => ws?.close();
    }

    function schedule() {
      if (closed) return;
      retry = setTimeout(connect, backoff);
      backoff = Math.min(backoff * 2, 10_000);
    }

    // A silent socket (sleeping laptop, dropped Wi-Fi) never fires onclose; watch for silence.
    const watchdog = setInterval(() => {
      if (ws && lastMsg.current && Date.now() - lastMsg.current > STALE_MS) {
        ws.close();
      }
    }, 2000);

    const offVis = onVisibility((v) => {
      if (v && pending && latest) {
        pending = false;
        const snap = latest;
        setState((s) => ({ ...s, lastMessageAt: lastMsg.current, snapshot: snap }));
      }
    });

    connect();
    return () => {
      closed = true;
      offVis();
      clearTimeout(retry);
      clearInterval(watchdog);
      ws?.close();
    };
  }, [machine.address, machine.token]);

  return state;
}

/** Re-render every `ms` (for countdowns and "x ago" labels). */
export function useNow(ms = 1000) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = setInterval(() => isVisible() && setNow(Date.now()), ms);
    const off = onVisibility((v) => v && setNow(Date.now()));
    return () => {
      clearInterval(t);
      off();
    };
  }, [ms]);
  return now;
}
