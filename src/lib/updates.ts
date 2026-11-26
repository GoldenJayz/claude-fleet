import { useCallback, useEffect, useRef, useState } from 'react';
import { isMobile, isTauri } from './machines';

/**
 * Auto-update from the project's GitHub releases. Windows: the Tauri updater (signature-checked)
 * installs and restarts. Android: the app downloads the APK, checks its SHA-256, and hands it to
 * Android's installer (which only accepts it if it's signed with the same key).
 */
export interface UpdateInfo {
  version: string;
  current: string;
  notes?: string | null;
}

export type UpdatePhase =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'upToDate' }
  | { kind: 'available'; info: UpdateInfo }
  | { kind: 'downloading'; info: UpdateInfo; got: number; total: number | null }
  | { kind: 'installing'; info: UpdateInfo }
  | { kind: 'needsPermission'; info: UpdateInfo; apk: string }
  | { kind: 'error'; info?: UpdateInfo; message: string };

declare global {
  interface Window {
    FleetAndroid?: { installApk(path: string): string; setWidgetMachines?(json: string): void };
  }
}

async function invoke<T>(cmd: string): Promise<T> {
  const core = await import('@tauri-apps/api/core');
  return core.invoke<T>(cmd);
}

const CHECK_EVERY = 6 * 3600_000;

export function useUpdates() {
  const [phase, setPhase] = useState<UpdatePhase>({ kind: 'idle' });
  const [dismissed, setDismissed] = useState<string | null>(null);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;
  const busy = () => ['downloading', 'installing', 'needsPermission'].includes(phaseRef.current.kind);

  const check = useCallback(async (manual = false) => {
    if (!isTauri || busy()) return;
    if (manual) setPhase({ kind: 'checking' });
    try {
      const info = await invoke<UpdateInfo | null>('check_update');
      if (busy()) return;
      if (info) setPhase({ kind: 'available', info });
      else if (manual) setPhase({ kind: 'upToDate' });
    } catch (e) {
      if (manual) setPhase({ kind: 'error', message: String(e) });
    }
  }, []);

  useEffect(() => {
    if (!isTauri) return;
    const cleanups: (() => void)[] = [];
    let alive = true;
    (async () => {
      const { listen } = await import('@tauri-apps/api/event');
      const add = (f: () => void) => (alive ? cleanups.push(f) : f());
      add(
        await listen<UpdateInfo>('fleet://update-available', (e) => {
          if (!busy()) setPhase({ kind: 'available', info: e.payload });
        }),
      );
      add(
        await listen<[number, number | null]>('fleet://update-progress', (e) => {
          setPhase((p) => (p.kind === 'downloading' ? { ...p, got: e.payload[0], total: e.payload[1] } : p));
        }),
      );
    })();
    const first = setTimeout(() => check(), 8000);
    const every = setInterval(() => check(), CHECK_EVERY);
    return () => {
      alive = false;
      clearTimeout(first);
      clearInterval(every);
      cleanups.forEach((f) => f());
    };
  }, [check]);

  const handToAndroid = (info: UpdateInfo, apk: string) => {
    const r = window.FleetAndroid?.installApk(apk);
    if (r === 'ok') setPhase({ kind: 'installing', info });
    else if (r === 'permission') setPhase({ kind: 'needsPermission', info, apk });
    else setPhase({ kind: 'error', info, message: 'Couldn’t start the Android installer.' });
  };

  const install = useCallback(async () => {
    const p = phaseRef.current;
    if (p.kind === 'needsPermission') return handToAndroid(p.info, p.apk);
    const info = p.kind === 'available' || p.kind === 'error' ? p.info : undefined;
    if (!info) return;
    setPhase({ kind: 'downloading', info, got: 0, total: null });
    try {
      const apk = await invoke<string | null>('install_update');
      // Windows restarts into the new version by itself.
      if (isMobile && typeof apk === 'string') handToAndroid(info, apk);
      else setPhase({ kind: 'installing', info });
    } catch (e) {
      setPhase({ kind: 'error', info, message: String(e) });
    }
  }, []);

  const visibleInfo = 'info' in phase ? phase.info : undefined;
  const showBanner = !!visibleInfo && !(phase.kind === 'available' && dismissed === visibleInfo.version);

  return { phase, check, install, showBanner, dismiss: () => visibleInfo && setDismissed(visibleInfo.version) };
}

export type Updates = ReturnType<typeof useUpdates>;
