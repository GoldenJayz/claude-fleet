import { motion } from 'framer-motion';
import { useCallback, useEffect, useRef, useState } from 'react';
import { httpBase } from '../lib/collector';
import { ago } from '../lib/format';
import type { MachineConfig } from '../types';

const btn = 'rounded-xl px-3 py-1.5 text-[12.5px] font-medium ring-1 transition-colors disabled:opacity-40';
const quiet = `${btn} bg-white/[0.08] text-white/85 ring-white/10 hover:bg-white/[0.14]`;

/** Full-window view of another PC's screen: a fresh screenshot on demand or every few seconds. */
export function ScreenView({ machine, name, from, now, onClose }: { machine: MachineConfig; name: string; from: string; now: number; onClose: () => void }) {
  const [url, setUrl] = useState<string | null>(null);
  const [takenAt, setTakenAt] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [auto, setAuto] = useState(false);
  const urlRef = useRef<string | null>(null);

  const grab = useCallback(async () => {
    setBusy(true);
    try {
      const r = await fetch(`${httpBase(machine)}/screen?from=${encodeURIComponent(from)}`, { headers: { Authorization: `Bearer ${machine.token}` } });
      if (!r.ok) {
        let msg = `HTTP ${r.status}`;
        try {
          msg = ((await r.json()) as { error?: string }).error ?? msg;
        } catch {
          /* keep status */
        }
        throw new Error(msg);
      }
      const blob = await r.blob();
      const next = URL.createObjectURL(blob);
      if (urlRef.current) URL.revokeObjectURL(urlRef.current);
      urlRef.current = next;
      setUrl(next);
      setTakenAt(Date.now());
      setErr(null);
    } catch (e) {
      setErr(e instanceof TypeError ? `Can’t reach ${name}.` : String((e as Error).message ?? e));
      setAuto(false);
    } finally {
      setBusy(false);
    }
  }, [machine, from, name]);

  useEffect(() => {
    grab();
    return () => {
      if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    };
  }, [grab]);
  useEffect(() => {
    if (!auto) return;
    let stop = false;
    let timer: ReturnType<typeof setTimeout>;
    const loop = async () => {
      await grab();
      if (!stop) timer = setTimeout(loop, 3000);
    };
    timer = setTimeout(loop, 3000);
    return () => {
      stop = true;
      clearTimeout(timer);
    };
  }, [auto, grab]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  const save = () => {
    if (!url) return;
    const a = document.createElement('a');
    a.href = url;
    a.download = `${name}-screen-${new Date(takenAt ?? Date.now()).toISOString().replace(/[:.]/g, '-')}.jpg`;
    a.click();
  };

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="fixed inset-0 z-50 flex flex-col gap-3 bg-[#07070c]/[0.97] p-3 md:p-5"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="mr-auto font-display text-lg font-semibold">
          {name}’s screen
          <span className="ml-2 text-[12px] font-normal text-white/45">{busy ? 'capturing…' : takenAt ? `taken ${ago(takenAt, now)}` : ''}</span>
        </h2>
        <button className={quiet} disabled={busy} onClick={grab}>
          Refresh
        </button>
        <button className={`${quiet} ${auto ? 'bg-violet-500/35 ring-violet-300/40' : ''}`} onClick={() => setAuto((a) => !a)} title="Take a new screenshot every 3 seconds">
          {auto ? 'Auto: on' : 'Auto'}
        </button>
        <button className={quiet} disabled={!url} onClick={save}>
          Save
        </button>
        <button className={quiet} onClick={onClose}>
          Close
        </button>
      </div>
      {err && <div className="rounded-xl bg-amber-500/10 px-3 py-2 text-[12.5px] text-amber-100">{err}</div>}
      <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto rounded-2xl bg-black/40 ring-1 ring-white/10 thin-scroll">
        {url ? (
          <img src={url} alt={`${name}'s screen`} className="max-h-full max-w-full object-contain" style={{ userSelect: 'none' }} />
        ) : (
          !err && <span className="text-[13px] text-white/45">Capturing…</span>
        )}
      </div>
    </motion.div>
  );
}
