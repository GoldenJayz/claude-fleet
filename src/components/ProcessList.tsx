import { motion } from 'framer-motion';
import { useCallback, useEffect, useState } from 'react';
import { killLog } from '../lib/collector';
import { ago, bytes, duration } from '../lib/format';
import type { HeavyProcess, HeavyRules, KillLogEntry, MachineConfig, ProcessInfo } from '../types';
import { Chip } from './Chip';
import { KillDialog, type KillTarget } from './KillDialog';

const endBtn =
  'rounded-lg px-2 py-0.5 text-[11.5px] font-medium text-rose-100/90 ring-1 ring-rose-300/30 bg-rose-500/10 hover:bg-rose-500/25 transition-colors';

function HeavyRow({ h, now, onEnd }: { h: HeavyProcess; now: number; onEnd: () => void }) {
  return (
    <motion.li
      layout="position"
      initial={{ opacity: 0, y: -4 }}
      animate={{ opacity: 1, y: 0 }}
      className="flex items-center gap-3 rounded-xl border border-amber-300/20 bg-amber-500/[0.07] px-3 py-2"
      title={h.exe ?? h.name}
    >
      <span className="h-2 w-2 shrink-0 rounded-full bg-amber-400 shadow-[0_0_8px_rgba(251,191,36,0.8)]" />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span className="truncate text-[13px] font-medium text-white/90">{h.name}</span>
          <span className="shrink-0 text-[10.5px] text-white/35 tabular">{h.pid}</span>
        </div>
        <div className="text-[11.5px] text-white/55 tabular">
          {h.reason.includes('cpu') && <span className="text-amber-100/90">{h.cpuAvg.toFixed(0)}% CPU</span>}
          {h.reason === 'cpu+memory' && ' · '}
          {h.reason.includes('memory') && <span className="text-amber-100/90">{bytes(h.memory)}</span>}
          {h.reason === 'cpu' && <span> · {bytes(h.memory)}</span>}
          <span className="text-white/40"> · for {duration(now - h.since)}</span>
        </div>
      </div>
      {h.protectedReason ? (
        <Chip tone="muted" title={`Never ended by Claude Fleet: ${h.protectedReason}`}>
          protected
        </Chip>
      ) : (
        <button className={endBtn} onClick={onEnd}>
          End…
        </button>
      )}
    </motion.li>
  );
}

function KillLogList({ machine, refreshKey, now }: { machine: MachineConfig; refreshKey: number; now: number }) {
  const [log, setLog] = useState<KillLogEntry[] | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    killLog(machine).then(setLog, (e: Error) => setErr(e.message));
  }, [machine, refreshKey]);
  if (err) return <div className="px-2 text-[12px] text-white/45">Couldn't load the log: {err}</div>;
  if (!log) return <div className="px-2 text-[12px] text-white/45">Loading…</div>;
  if (!log.length) return <div className="px-2 py-4 text-[12.5px] text-white/45">Nothing has been ended on this machine.</div>;
  return (
    <ul className="flex flex-col gap-1.5">
      {log.map((e) => (
        <li key={e.id} className="glass-tile rounded-xl px-3 py-2 text-[12px]">
          <div className="flex items-center gap-2">
            <span className={`h-1.5 w-1.5 rounded-full ${e.ok ? 'bg-emerald-400' : 'bg-rose-400'}`} />
            <span className="truncate font-medium text-white/85">{e.name}</span>
            <span className="text-white/35 tabular">{e.pid}</span>
            {e.tree && <Chip tone="muted">tree</Chip>}
            <span className="ml-auto shrink-0 text-white/40" title={new Date(e.ts).toLocaleString()}>
              {ago(e.ts, now)}
            </span>
          </div>
          <div className="mt-0.5 truncate text-white/45" title={e.detail ?? undefined}>
            by {e.requestedBy}
            {e.detail ? ` · ${e.detail}` : ''}
          </div>
        </li>
      ))}
    </ul>
  );
}

export function ProcessList({
  machine,
  machineName,
  processes,
  heavy,
  rules,
  totalMemory,
  requestedBy,
  now,
  offline,
}: {
  machine: MachineConfig;
  machineName: string;
  processes: ProcessInfo[];
  heavy: HeavyProcess[];
  rules: HeavyRules;
  totalMemory: number;
  requestedBy: string;
  now: number;
  offline: boolean;
}) {
  const [tab, setTab] = useState<'live' | 'log'>('live');
  const [target, setTarget] = useState<KillTarget | null>(null);
  const [logKey, setLogKey] = useState(0);
  const heavyPids = new Set(heavy.map((h) => h.pid));
  const rows = processes.filter((p) => !heavyPids.has(p.pid)).slice(0, 14);

  const close = useCallback((changed: boolean) => {
    setTarget(null);
    if (changed) setLogKey((k) => k + 1);
  }, []);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="mb-3 flex gap-1 self-start rounded-xl bg-white/[0.06] p-0.5 ring-1 ring-white/10">
        {(['live', 'log'] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`rounded-[10px] px-3 py-1 text-[12px] ${tab === t ? 'bg-white/15 text-white' : 'text-white/55 hover:text-white'}`}
          >
            {t === 'live' ? 'Live' : 'Kill log'}
          </button>
        ))}
      </div>

      {tab === 'log' ? (
        <div className="thin-scroll -mr-2 min-h-0 flex-1 overflow-y-auto pr-2">
          <KillLogList machine={machine} refreshKey={logKey} now={now} />
        </div>
      ) : (
        <div className="thin-scroll -mr-2 flex min-h-0 flex-1 flex-col overflow-y-auto pr-2">
          <div className="mb-1.5 flex items-baseline justify-between px-1">
            <span className="text-[10px] font-medium uppercase tracking-[0.14em] text-amber-200/80">Heavy</span>
            <span className="text-[10.5px] text-white/35" title="A process counts as heavy only when it stays above this for the whole period">
              ≥{rules.cpuPct}% CPU or ≥{bytes(rules.memoryBytes, 0)} for {Math.round(rules.sustainMs / 1000)}s+
            </span>
          </div>
          <ul className="mb-4 flex flex-col gap-1.5">
            {heavy.map((h) => (
              <HeavyRow key={`${h.pid}:${h.startTime}`} h={h} now={now} onEnd={() => setTarget({ pid: h.pid, startTime: h.startTime, name: h.name })} />
            ))}
            {heavy.length === 0 && <li className="px-1 text-[12.5px] text-white/45">Nothing heavy right now.</li>}
          </ul>

          <div className="mb-1.5 grid grid-cols-[1fr_84px_66px_44px] gap-2 px-2 text-[10px] font-medium uppercase tracking-[0.14em] text-white/40">
            <span>Top processes</span>
            <span className="text-right">CPU</span>
            <span className="text-right">Memory</span>
            <span />
          </div>
          <ul>
            {rows.map((p) => (
              <li
                key={p.pid}
                className="group grid grid-cols-[1fr_84px_66px_44px] items-center gap-2 rounded-xl px-2 py-1.5 text-[12.5px] hover:bg-white/[0.04]"
                title={p.exe ?? p.name}
              >
                <span className="flex min-w-0 items-baseline gap-2">
                  <span className="truncate text-white/85">{p.name}</span>
                  <span className="shrink-0 text-[10.5px] text-white/30 tabular">{p.pid}</span>
                </span>
                <span className="flex items-center justify-end gap-2 tabular">
                  <span className="h-1 w-7 overflow-hidden rounded-full bg-white/[0.07]">
                    <motion.span className="block h-full rounded-full bg-gradient-to-r from-cyan-300 to-blue-500" animate={{ width: `${Math.min(100, p.cpu)}%` }} />
                  </span>
                  <span className="w-10 text-right text-white/75">{p.cpu.toFixed(1)}%</span>
                </span>
                <span className="text-right text-white/75 tabular" title={`${((p.memory * 100) / Math.max(1, totalMemory)).toFixed(1)}% of RAM`}>
                  {bytes(p.memory, 0)}
                </span>
                <span className="flex justify-end">
                  {!offline && (
                    <button
                      className={`${endBtn} opacity-0 group-hover:opacity-100 focus:opacity-100`}
                      onClick={() => setTarget({ pid: p.pid, startTime: p.startTime, name: p.name })}
                    >
                      End
                    </button>
                  )}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <KillDialog machine={machine} machineName={machineName} target={target} requestedBy={requestedBy} onClose={close} />
    </div>
  );
}
