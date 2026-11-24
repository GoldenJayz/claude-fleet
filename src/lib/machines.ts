import { useCallback, useEffect, useMemo, useState } from 'react';
import type { MachineConfig, MachineInfo } from '../types';

declare const __DEV_LOCAL__: { token: string; port: number; name: string } | null;

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
/** A phone or tablet (the Android app, or a phone browser): touch layout, no local collector. */
export const isMobile = typeof navigator !== 'undefined' && /Android|iPhone|iPad/i.test(navigator.userAgent);
/** The Windows app, which runs this PC's collector and can change Windows settings. */
export const isDesktopApp = isTauri && !isMobile;
export const DEFAULT_PORT = 47821;

export interface LocalCollector {
  machineName: string;
  port: number;
  token: string;
  embedded: boolean;
  error: string | null;
  dataDir: string;
}

export interface LanAddress {
  ip: string;
  interface: string;
  kind: 'lan' | 'tailscale' | 'virtual';
}

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const core = await import('@tauri-apps/api/core');
  return core.invoke<T>(cmd, args);
}

export async function loadLocal(): Promise<LocalCollector | null> {
  if (isDesktopApp) return invoke<LocalCollector>('local_collector');
  // The phone app only views other machines.
  if (isTauri) return null;
  // Plain browser during `npm run dev`: the dev server injected this machine's collector details.
  if (__DEV_LOCAL__) {
    return { machineName: __DEV_LOCAL__.name, port: __DEV_LOCAL__.port, token: __DEV_LOCAL__.token, embedded: false, error: null, dataDir: '' };
  }
  return null;
}

/** The phone app keeps its list in its own private storage; a dev browser uses a separate key. */
const STORE_KEY = isTauri ? 'claude-fleet.machines' : 'claude-fleet.machines.dev';

/** Saved remote machines: `%APPDATA%\ClaudeFleet\dashboard.json` on Windows, page storage elsewhere. */
async function loadSaved(): Promise<MachineConfig[]> {
  if (isDesktopApp) return invoke<MachineConfig[]>('get_machines');
  try {
    return JSON.parse(localStorage.getItem(STORE_KEY) ?? '[]');
  } catch {
    return [];
  }
}

async function storeSaved(list: MachineConfig[]) {
  const clean = list.map(({ id, name, address, token }) => ({ id, name, address, token }));
  if (isDesktopApp) await invoke('save_machines', { machines: clean });
  else localStorage.setItem(STORE_KEY, JSON.stringify(clean));
}

export async function loadAddresses(): Promise<LanAddress[]> {
  return isDesktopApp ? invoke<LanAddress[]>('local_addresses') : [];
}

// ---------- pairing codes ----------

/** `claude-fleet://192.168.1.20:47821/<token>?name=DesktopPC`: everything another PC needs, in one line. */
export function pairingCode(address: string, token: string, name: string) {
  return `claude-fleet://${address}/${token}?name=${encodeURIComponent(name)}`;
}

export function parsePairingCode(text: string): { address: string; token: string; name?: string } | null {
  const m = /claude-fleet:\/\/([^/\s]+)\/([0-9a-fA-F]{32,})(?:\?name=([^\s]*))?/.exec(text.trim());
  if (!m) return null;
  return { address: normalizeAddress(m[1]), token: m[2], name: m[3] ? decodeURIComponent(m[3]) : undefined };
}

/** "192.168.1.20" → "192.168.1.20:47821"; strips http:// and trailing slashes. */
export function normalizeAddress(a: string) {
  let s = a.trim().replace(/^https?:\/\//, '').replace(/\/.*$/, '');
  if (!s) return s;
  if (!/:\d+$/.test(s)) s = `${s}:${DEFAULT_PORT}`;
  return s;
}

export type TestResult = { ok: true; info: MachineInfo } | { ok: false; error: string };

/** Contact a collector with a token; explains failures in plain words. */
export async function testMachine(address: string, token: string): Promise<TestResult> {
  const ctl = new AbortController();
  const timer = setTimeout(() => ctl.abort(), 5000);
  try {
    const r = await fetch(`http://${address}/v1/info`, { headers: { Authorization: `Bearer ${token}` }, signal: ctl.signal });
    if (r.status === 401) return { ok: false, error: 'Reached the PC, but the pairing token is wrong.' };
    if (r.status === 403) return { ok: false, error: 'The collector refused this address (only LAN and Tailscale addresses are allowed).' };
    if (!r.ok) return { ok: false, error: `The collector answered with HTTP ${r.status}.` };
    return { ok: true, info: (await r.json()) as MachineInfo };
  } catch {
    return {
      ok: false,
      error: `Couldn't reach ${address}. Check that Claude Fleet is running there, the address is right, and Windows Firewall allows port ${address.split(':')[1] ?? DEFAULT_PORT}.`,
    };
  } finally {
    clearTimeout(timer);
  }
}

// ---------- hook ----------

/** This PC (always first) plus the saved machines. */
export function useMachines() {
  const [local, setLocal] = useState<LocalCollector | null>(null);
  const [saved, setSaved] = useState<MachineConfig[]>([]);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    Promise.all([loadLocal(), loadSaved()])
      .then(([l, s]) => {
        setLocal(l);
        setSaved(s);
      })
      .finally(() => setLoaded(true));
  }, []);

  const machines = useMemo<MachineConfig[]>(() => {
    const list: MachineConfig[] = [];
    if (local?.token) list.push({ id: 'local', name: local.machineName, address: `127.0.0.1:${local.port}`, token: local.token, isLocal: true });
    return [...list, ...saved];
  }, [local, saved]);

  const updateSaved = useCallback(async (next: MachineConfig[]) => {
    await storeSaved(next);
    setSaved(next);
  }, []);

  return { local, machines, saved, updateSaved, loaded };
}
