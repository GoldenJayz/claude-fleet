import { useEffect, useRef, useState } from 'react';
import type { ConnectionStatus, KillImpact, KillLogEntry, KillResult, MachineConfig, MetricsPoint, Snapshot, StreamEvent, TokenTotals } from '../types';

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
  }
}

/** Live connection to one collector: WebSocket stream with reconnect and offline detection. */
export function useCollector(machine: MachineConfig): CollectorState {
  const [state, setState] = useState<CollectorState>({ status: 'connecting', snapshot: null, lastMessageAt: null, error: null });
  const lastMsg = useRef(0);

  useEffect(() => {
    let ws: WebSocket | null = null;
    let closed = false;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let backoff = 1000;

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
        setState((s) => ({ status: 'online', error: null, lastMessageAt: lastMsg.current, snapshot: apply(s.snapshot, ev) }));
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

    connect();
    return () => {
      closed = true;
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
    const t = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(t);
  }, [ms]);
  return now;
}
