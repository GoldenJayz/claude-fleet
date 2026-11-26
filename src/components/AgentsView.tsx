import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useMemo, useRef, useState } from 'react';
import { agentStatus, cancelTask, folderName, getTask, isActive, MODELS, MODES, onTaskStep, submitTask } from '../lib/agent';
import type { CollectorState } from '../lib/collector';
import { ago, duration } from '../lib/format';
import { isMobile } from '../lib/machines';
import type { AgentMode, AgentStatus, AgentTask, MachineConfig } from '../types';
import { Chip } from './Chip';

const input =
  'w-full rounded-xl bg-black/30 px-3 py-2 text-[13px] text-white/90 ring-1 ring-white/10 outline-none placeholder:text-white/30 focus:ring-violet-300/40';
const btn = 'rounded-xl px-3 py-1.5 text-[12.5px] font-medium ring-1 transition-colors disabled:opacity-40';
const primary = `${btn} bg-violet-500/35 text-violet-50 ring-violet-300/40 hover:bg-violet-500/50`;
const quiet = `${btn} bg-white/[0.06] text-white/80 ring-white/10 hover:bg-white/[0.12]`;

function StatusChip({ t }: { t: AgentTask }) {
  switch (t.status) {
    case 'queued':
      return <Chip tone="muted">queued</Chip>;
    case 'running':
      return (
        <Chip tone="medium">
          <span className="live-dot h-1.5 w-1.5 rounded-full bg-violet-300" /> working
        </Chip>
      );
    case 'done':
      return <Chip tone="live">done</Chip>;
    case 'cancelled':
      return <Chip tone="muted">cancelled</Chip>;
    default:
      return <Chip tone="bad">failed</Chip>;
  }
}

type Row = { machine: MachineConfig; name: string; task: AgentTask };

// ---------- composer ----------

function Composer({ machines, states, from }: { machines: MachineConfig[]; states: Record<string, CollectorState>; from: string }) {
  const capable = machines.filter((m) => states[m.id]?.status === 'online' && states[m.id]?.snapshot?.machine.features?.includes('agent'));
  const [machineId, setMachineId] = useState<string>('');
  const machine = capable.find((m) => m.id === machineId) ?? capable[0];
  const [status, setStatus] = useState<AgentStatus | null>(null);
  const [statusErr, setStatusErr] = useState<string | null>(null);
  const [cwd, setCwd] = useState('');
  const [mode, setMode] = useState<AgentMode>('acceptEdits');
  const [model, setModel] = useState('');
  const [prompt, setPrompt] = useState('');
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);

  useEffect(() => {
    setStatus(null);
    setStatusErr(null);
    if (!machine) return;
    agentStatus(machine)
      .then((s) => {
        setStatus(s);
        setCwd((c) => c || s.folders[0] || s.home || '');
      })
      .catch((e) => setStatusErr(String(e.message ?? e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [machine?.id]);

  if (!capable.length) {
    return (
      <section className="glass rounded-3xl p-5 text-[13px] text-white/55">
        No PC that can take tasks is online. Machines need Pane 0.3 or newer, and must be online.
      </section>
    );
  }
  const off = status?.access === 'off';
  const send = async () => {
    if (!machine || !prompt.trim() || !cwd.trim()) return;
    setBusy(true);
    setMsg(null);
    try {
      const t = await submitTask(machine, { prompt, cwd: cwd.trim(), mode, model: model || undefined, from });
      setPrompt('');
      setMsg({ ok: true, text: t.status === 'queued' && status?.running ? 'Queued: it starts when the current task finishes.' : 'Sent.' });
    } catch (e) {
      setMsg({ ok: false, text: String((e as Error).message ?? e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="glass flex flex-col gap-3 rounded-3xl p-4 md:p-5">
      <div className="flex flex-wrap items-center gap-2">
        <h2 className="mr-auto font-display text-[15px] font-semibold">New task</h2>
        <select value={machine?.id} onChange={(e) => setMachineId(e.target.value)} className={`${input} w-auto py-1.5`}>
          {capable.map((m) => (
            <option key={m.id} value={m.id} className="bg-neutral-900">
              {states[m.id]?.snapshot?.machine.machineName ?? m.name}
            </option>
          ))}
        </select>
      </div>

      {statusErr && <div className="rounded-xl bg-rose-500/10 px-3 py-2 text-[12.5px] text-rose-100">{statusErr}</div>}
      {off && (
        <div className="rounded-xl bg-amber-500/10 px-3 py-2 text-[12.5px] text-amber-100">
          Agent tasks are turned off on {status?.machine}. On that PC, open Pane → Settings → System → Agent tasks.
        </div>
      )}

      <textarea
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) send();
        }}
        rows={isMobile ? 4 : 3}
        disabled={off}
        placeholder={`What should Claude do on ${status?.machine ?? 'that PC'}?`}
        className={`${input} resize-y select-text leading-snug`}
      />

      <div className="grid gap-2 md:grid-cols-[1fr_auto_auto]">
        <div>
          <input list="agent-folders" value={cwd} onChange={(e) => setCwd(e.target.value)} placeholder="Folder on that PC, e.g. C:\Users\you\project" className={`${input} font-mono text-[12px]`} />
          <datalist id="agent-folders">
            {status?.folders.map((f) => (
              <option key={f} value={f} />
            ))}
          </datalist>
        </div>
        <select value={mode} onChange={(e) => setMode(e.target.value as AgentMode)} className={`${input} w-auto`} title={MODES.find((x) => x.value === mode)?.hint}>
          {MODES.map((x) => (
            <option key={x.value} value={x.value} disabled={x.value === 'bypassPermissions' && status?.access !== 'full'} className="bg-neutral-900">
              {x.label}
            </option>
          ))}
        </select>
        <select value={model} onChange={(e) => setModel(e.target.value)} className={`${input} w-auto`}>
          {MODELS.map((x) => (
            <option key={x.value} value={x.value} className="bg-neutral-900">
              {x.label}
            </option>
          ))}
        </select>
      </div>
      {status?.folders.length ? (
        <div className="flex flex-wrap gap-1.5">
          {status.folders.slice(0, 6).map((f) => (
            <button key={f} onClick={() => setCwd(f)} title={f} className={`rounded-lg px-2 py-0.5 text-[11.5px] ring-1 ${cwd === f ? 'bg-white/15 text-white ring-white/20' : 'text-white/55 ring-white/10 hover:text-white'}`}>
              {folderName(f)}
            </button>
          ))}
        </div>
      ) : null}

      <div className="flex items-center gap-3">
        <span className="min-w-0 flex-1 text-[11.5px] text-white/40">{MODES.find((x) => x.value === mode)?.hint}</span>
        {msg && <span className={`text-[12px] ${msg.ok ? 'text-emerald-200' : 'text-rose-200'}`}>{msg.text}</span>}
        <button className={primary} disabled={busy || off || !prompt.trim() || !cwd.trim()} onClick={send}>
          {status?.running ? 'Queue' : 'Send'}
        </button>
      </div>
    </section>
  );
}

// ---------- one task ----------

function TaskDetail({ row, from, now, canReply }: { row: Row; from: string; now: number; canReply: boolean }) {
  const { machine, task } = row;
  const [full, setFull] = useState<AgentTask | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [reply, setReply] = useState('');
  const [busy, setBusy] = useState(false);
  const bottom = useRef<HTMLDivElement>(null);

  // Load all steps, then append live ones; reload when the task changes state (e.g. finishes).
  useEffect(() => {
    let alive = true;
    getTask(machine, task.id)
      .then((t) => alive && setFull(t))
      .catch((e) => alive && setErr(String(e.message ?? e)));
    return () => {
      alive = false;
    };
  }, [machine, task.id, task.status]);
  useEffect(
    () =>
      onTaskStep((mid, tid, step) => {
        if (mid === machine.id && tid === task.id) setFull((f) => (f ? { ...f, steps: [...f.steps, step] } : f));
      }),
    [machine.id, task.id],
  );
  useEffect(() => {
    if (isActive(task)) bottom.current?.scrollIntoView({ block: 'nearest' });
  }, [full?.steps.length, task]);

  const t = { ...task, ...(full ? { steps: full.steps, result: full.result ?? task.result } : {}) };
  const queueReply = async () => {
    if (!reply.trim()) return;
    setBusy(true);
    setErr(null);
    try {
      await submitTask(machine, { prompt: reply, cwd: t.cwd, mode: t.mode, model: t.model ?? undefined, followUpOf: t.id, from });
      setReply('');
    } catch (e) {
      setErr(String((e as Error).message ?? e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-2 border-t border-white/10 px-1 pt-3">
      <div className="max-h-[50vh] select-text overflow-y-auto rounded-xl bg-black/25 p-3 font-mono text-[12px] leading-relaxed thin-scroll">
        <div className="mb-2 whitespace-pre-wrap text-white/85">
          <span className="text-violet-300">› </span>
          {t.prompt}
        </div>
        {(t.steps ?? []).map((s, i) => (
          <div
            key={i}
            className={`whitespace-pre-wrap ${
              s.kind === 'tool' ? 'text-sky-200/80' : s.kind === 'text' ? 'font-sans text-[12.5px] text-white/85' : s.kind === 'note' ? 'text-white/35' : 'text-rose-200'
            } ${s.kind === 'text' ? 'my-1.5' : ''}`}
          >
            {s.kind === 'tool' ? '⏵ ' : s.kind === 'toolError' || s.kind === 'error' ? '✕ ' : ''}
            {s.text}
          </div>
        ))}
        {t.status === 'queued' && <div className="text-white/40">Waiting for the task before it to finish…</div>}
        {t.status === 'running' && <div className="text-violet-200/70">Working…</div>}
        {t.error && !t.steps?.some((s) => s.kind === 'error') && <div className="text-rose-200">✕ {t.error}</div>}
        <div ref={bottom} />
      </div>
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11.5px] text-white/40 tabular">
        <span title={t.cwd}>{t.cwd}</span>
        <span>{MODES.find((x) => x.value === t.mode)?.label}</span>
        {t.model && <span>{t.model}</span>}
        {t.turns != null && <span>{t.turns} turns</span>}
        {t.costUsd != null && <span>${t.costUsd.toFixed(3)}</span>}
        {t.started && <span>{duration((t.ended ?? now) - t.started)}</span>}
        <span>from {t.from}</span>
      </div>
      {err && <div className="text-[12px] text-rose-200">{err}</div>}
      <div className={`flex gap-2 ${canReply ? '' : 'hidden'}`}>
        <input
          value={reply}
          onChange={(e) => setReply(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && queueReply()}
          placeholder={isActive(t) ? 'Queue a follow-up message…' : 'Follow up…'}
          className={input}
        />
        <button className={primary} disabled={busy || !reply.trim()} onClick={queueReply}>
          {isActive(t) ? 'Queue' : 'Send'}
        </button>
        {isActive(t) && (
          <button className={`${quiet} hover:bg-rose-500/30`} onClick={() => cancelTask(machine, t.id).catch((e) => setErr(String(e.message ?? e)))}>
            {t.status === 'queued' ? 'Remove' : 'Stop'}
          </button>
        )}
      </div>
    </div>
  );
}

// ---------- page ----------

export function AgentsView({
  machines,
  states,
  from,
  now,
}: {
  machines: MachineConfig[];
  states: Record<string, CollectorState>;
  from: string;
  now: number;
}) {
  const [open, setOpen] = useState<string | null>(null);
  const rows: Row[] = useMemo(
    () =>
      machines
        .flatMap((m) => (states[m.id]?.snapshot?.tasks ?? []).map((task) => ({ machine: m, name: states[m.id]?.snapshot?.machine.machineName ?? m.name, task })))
        .sort((a, b) => b.task.created - a.task.created),
    [machines, states],
  );
  // Follow-ups are shown inside the task they continue.
  const threads = useMemo(() => {
    const top: Row[] = [];
    const children = new Map<string, Row[]>();
    const key = (r: Row, id: number) => `${r.machine.id}:${id}`;
    for (const r of rows) {
      if (r.task.followUpOf) {
        const k = key(r, r.task.followUpOf);
        children.set(k, [...(children.get(k) ?? []), r]);
      } else top.push(r);
    }
    const flatten = (r: Row): Row[] => [r, ...(children.get(key(r, r.task.id)) ?? []).sort((a, b) => a.task.id - b.task.id).flatMap(flatten)];
    return top.map((r) => ({ root: r, all: flatten(r) }));
  }, [rows]);

  return (
    <div className="mx-auto flex w-full max-w-4xl flex-col gap-3 md:gap-4">
      <Composer machines={machines} states={states} from={from} />
      <section className="glass rounded-3xl p-3 md:p-4">
        <h2 className="mb-2 px-1 font-display text-[15px] font-semibold">Tasks</h2>
        {!threads.length && <div className="px-1 pb-2 text-[12.5px] text-white/45">Nothing yet. Tasks you send show up here with their progress.</div>}
        <ul className="flex flex-col gap-1.5">
          {threads.map(({ root, all }) => {
            const k = `${root.machine.id}:${root.task.id}`;
            const last = all[all.length - 1];
            const active = all.find((r) => isActive(r.task)) ?? last;
            return (
              <li key={k} className="glass-tile rounded-2xl px-3 py-2.5">
                <button className="flex w-full items-center gap-3 text-left" onClick={() => setOpen(open === k ? null : k)}>
                  <StatusChip t={active.task} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] text-white/90">{root.task.prompt.split('\n')[0]}</span>
                    <span className="block truncate text-[11.5px] text-white/40">
                      {root.name} · {folderName(root.task.cwd)} · {ago(last.task.created, now)}
                      {all.length > 1 ? ` · ${all.length} messages` : ''}
                    </span>
                  </span>
                </button>
                <AnimatePresence initial={false}>
                  {open === k && (
                    <motion.div initial={{ height: 0, opacity: 0 }} animate={{ height: 'auto', opacity: 1 }} exit={{ height: 0, opacity: 0 }} className="overflow-hidden">
                      <div className="mt-2 flex flex-col gap-3">
                        {all.map((r) => (
                          <TaskDetail key={r.task.id} row={r} from={from} now={now} canReply={r === last} />
                        ))}
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>
              </li>
            );
          })}
        </ul>
      </section>
    </div>
  );
}
