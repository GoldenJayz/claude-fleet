import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { killImpact, killProcess } from '../lib/collector';
import { bytes } from '../lib/format';
import type { KillImpact, KillResult, MachineConfig } from '../types';

export interface KillTarget {
  pid: number;
  startTime: number;
  name: string;
}

const btn = 'rounded-xl px-4 py-2 text-[13px] font-medium ring-1 transition-colors disabled:opacity-40';

/**
 * Confirmation before ending anything: shows the process, what it would free, and offers
 * "End process" or "End process tree". Protected processes can't be ended from here.
 */
export function KillDialog({
  machine,
  machineName,
  target,
  requestedBy,
  onClose,
}: {
  machine: MachineConfig;
  machineName: string;
  target: KillTarget | null;
  requestedBy: string;
  onClose: (changed: boolean) => void;
}) {
  const [impact, setImpact] = useState<KillImpact | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<'one' | 'tree' | null>(null);
  const [result, setResult] = useState<KillResult | null>(null);

  useEffect(() => {
    setImpact(null);
    setError(null);
    setResult(null);
    setBusy(null);
    if (!target) return;
    killImpact(machine, target.pid, target.startTime).then(setImpact, (e: Error) => setError(e.message));
  }, [machine, target]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && !busy && onClose(!!result);
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [busy, result, onClose]);

  async function run(tree: boolean) {
    if (!target) return;
    setBusy(tree ? 'tree' : 'one');
    setError(null);
    try {
      setResult(await killProcess(machine, target.pid, target.startTime, tree, requestedBy));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(null);
    }
  }

  const needsAdmin = result?.outcomes.some((o) => o.needsAdmin);
  const freed = result?.outcomes.filter((o) => o.ok).reduce((a, o) => a + o.memory, 0) ?? 0;

  return createPortal(
    <AnimatePresence>
      {target && (
        <motion.div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-6"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onMouseDown={(e) => e.target === e.currentTarget && !busy && onClose(!!result)}
        >
          <motion.div
            role="dialog"
            aria-modal="true"
            initial={{ scale: 0.96, y: 8 }}
            animate={{ scale: 1, y: 0 }}
            exit={{ scale: 0.97, opacity: 0 }}
            transition={{ type: 'spring', stiffness: 300, damping: 26 }}
            className="glass w-full max-w-[520px] rounded-3xl p-6"
            style={{ background: 'rgba(18,18,30,0.92)' }}
          >
            <div className="text-[12px] uppercase tracking-[0.14em] text-white/45">End process on {machineName}</div>
            <h2 className="mt-1 truncate font-display text-xl font-semibold">{target.name}</h2>
            <div className="mt-0.5 truncate text-[12px] text-white/45 tabular" title={impact?.exe ?? undefined}>
              PID {target.pid}
              {impact?.exe ? ` · ${impact.exe}` : ''}
            </div>

            {!impact && !error && <div className="mt-5 text-[13px] text-white/55">Checking what this would free…</div>}

            {impact && !result && (
              <>
                {impact.protectedReason ? (
                  <div className="mt-5 rounded-2xl bg-amber-500/10 px-4 py-3 text-[13px] text-amber-100 ring-1 ring-amber-300/25">
                    This process is protected ({impact.protectedReason}) and can't be ended from Claude Fleet.
                  </div>
                ) : (
                  <div className="mt-5 grid grid-cols-2 gap-3">
                    <div className="glass-tile rounded-2xl p-3.5">
                      <div className="text-[11px] uppercase tracking-[0.12em] text-white/45">This process</div>
                      <div className="mt-1 font-display text-lg font-semibold tabular">{bytes(impact.memory)}</div>
                      <div className="text-[12px] text-white/55 tabular">{impact.cpu.toFixed(1)}% CPU</div>
                    </div>
                    <div className="glass-tile rounded-2xl p-3.5">
                      <div className="text-[11px] uppercase tracking-[0.12em] text-white/45">
                        Whole tree · {impact.treeCount} process{impact.treeCount === 1 ? '' : 'es'}
                      </div>
                      <div className="mt-1 font-display text-lg font-semibold tabular">{bytes(impact.treeMemory)}</div>
                      <div className="text-[12px] text-white/55 tabular">{impact.treeCpu.toFixed(1)}% CPU</div>
                    </div>
                  </div>
                )}
                {impact.treeProtected.length > 0 && !impact.protectedReason && (
                  <div className="mt-3 text-[12px] text-white/50">
                    “End process tree” leaves these protected processes running: {impact.treeProtected.join(', ')}.
                  </div>
                )}
                <div className="mt-3 text-[12px] text-white/45">
                  Unsaved work in this program will be lost. Windows ends it immediately, without asking it to close.
                </div>
              </>
            )}

            {result && (
              <div className="mt-5 flex flex-col gap-2">
                <div
                  className={`rounded-2xl px-4 py-3 text-[13px] ring-1 ${
                    result.ok ? 'bg-emerald-500/10 text-emerald-100 ring-emerald-300/25' : 'bg-rose-500/10 text-rose-100 ring-rose-300/25'
                  }`}
                >
                  {result.ok
                    ? `Ended ${result.outcomes.filter((o) => o.ok).length} process${result.outcomes.filter((o) => o.ok).length === 1 ? '' : 'es'}, freeing about ${bytes(freed)}.`
                    : needsAdmin
                      ? `Windows needs administrator rights to end ${target.name}. Claude Fleet doesn't run as admin, so use Task Manager (run as administrator) for this one.`
                      : `Couldn't end ${target.name}.`}
                </div>
                {result.outcomes.filter((o) => !o.ok).map((o) => (
                  <div key={o.pid} className="text-[12px] text-white/55">
                    {o.name} ({o.pid}): {o.error}
                  </div>
                ))}
                {result.skippedProtected.length > 0 && (
                  <div className="text-[12px] text-white/50">Left running (protected): {result.skippedProtected.join(', ')}</div>
                )}
              </div>
            )}

            {error && <div className="mt-5 rounded-2xl bg-rose-500/10 px-4 py-3 text-[13px] text-rose-100 ring-1 ring-rose-300/25">{error}</div>}

            <div className="mt-6 flex justify-end gap-2">
              {result || error || impact?.protectedReason ? (
                <button className={`${btn} bg-white/[0.08] text-white/85 ring-white/10 hover:bg-white/[0.14]`} onClick={() => onClose(!!result)}>
                  Close
                </button>
              ) : (
                <>
                  <button className={`${btn} bg-white/[0.06] text-white/80 ring-white/10 hover:bg-white/[0.12]`} disabled={!!busy} onClick={() => onClose(false)}>
                    Cancel
                  </button>
                  <button
                    className={`${btn} bg-rose-500/20 text-rose-50 ring-rose-300/35 hover:bg-rose-500/35`}
                    disabled={!impact || !!busy}
                    onClick={() => run(false)}
                  >
                    {busy === 'one' ? 'Ending…' : 'End process'}
                  </button>
                  <button
                    className={`${btn} bg-rose-600/35 text-white ring-rose-300/45 hover:bg-rose-600/50`}
                    disabled={!impact || !!busy}
                    onClick={() => run(true)}
                    title={impact ? `Ends ${impact.treeCount} process${impact.treeCount === 1 ? '' : 'es'}, children first` : undefined}
                  >
                    {busy === 'tree' ? 'Ending…' : 'End process tree'}
                  </button>
                </>
              )}
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}
