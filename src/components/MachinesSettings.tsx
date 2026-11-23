import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useState } from 'react';
import type { CollectorState } from '../lib/collector';
import { normalizeAddress, parsePairingCode, testMachine, type TestResult } from '../lib/machines';
import type { MachineConfig } from '../types';
import { Chip } from './Chip';

const input =
  'w-full rounded-xl bg-black/30 px-3 py-2 text-[13px] text-white/90 ring-1 ring-white/10 outline-none placeholder:text-white/30 focus:ring-violet-300/50 select-text';
const btn = 'rounded-xl px-3.5 py-2 text-[12.5px] font-medium ring-1 transition-colors disabled:opacity-40';
const primary = `${btn} bg-violet-500/30 text-violet-50 ring-violet-300/40 hover:bg-violet-500/45`;
const quiet = `${btn} bg-white/[0.06] text-white/80 ring-white/10 hover:bg-white/[0.12]`;

function StatusChip({ s }: { s?: CollectorState }) {
  if (!s) return <Chip tone="muted">not connected</Chip>;
  if (s.status === 'online') return <Chip tone="live">online</Chip>;
  if (s.status === 'connecting') return <Chip tone="muted">connecting…</Chip>;
  if (s.status === 'unauthorized') return <Chip tone="bad">token rejected</Chip>;
  return <Chip tone="bad" title={s.error ?? undefined}>offline</Chip>;
}

/** Add or edit one machine. Accepts a pasted pairing code or separate address + token. */
function MachineForm({
  initial,
  existing,
  onSave,
  onCancel,
}: {
  initial?: MachineConfig;
  existing: MachineConfig[];
  onSave: (m: MachineConfig) => Promise<void>;
  onCancel: () => void;
}) {
  const [code, setCode] = useState('');
  const [name, setName] = useState(initial?.name ?? '');
  const [address, setAddress] = useState(initial?.address ?? '');
  const [token, setToken] = useState(initial?.token ?? '');
  const [test, setTest] = useState<TestResult | null>(null);
  const [busy, setBusy] = useState(false);

  // Pasting a pairing code fills everything in.
  useEffect(() => {
    const p = parsePairingCode(code);
    if (p) {
      setAddress(p.address);
      setToken(p.token);
      if (p.name && !name) setName(p.name);
      setTest(null);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [code]);

  const addr = normalizeAddress(address);
  const duplicate = existing.find((m) => m.address === addr && m.id !== initial?.id);
  const canTest = addr && token.trim().length >= 32;

  async function run() {
    setBusy(true);
    const r = await testMachine(addr, token.trim());
    setTest(r);
    if (r.ok && !name) setName(r.info.machineName);
    setBusy(false);
    return r;
  }

  async function save() {
    setBusy(true);
    const r = test?.ok ? test : await testMachine(addr, token.trim());
    setTest(r);
    if (r.ok) {
      await onSave({ id: initial?.id ?? `m${Date.now().toString(36)}`, name: name.trim() || r.info.machineName, address: addr, token: token.trim() });
    }
    setBusy(false);
  }

  return (
    <motion.div
      initial={{ opacity: 0, height: 0 }}
      animate={{ opacity: 1, height: 'auto' }}
      exit={{ opacity: 0, height: 0 }}
      className="overflow-hidden"
    >
      <div className="glass-tile flex flex-col gap-3 rounded-2xl p-4">
        {!initial && (
          <label className="flex flex-col gap-1.5">
            <span className="text-[12px] text-white/60">Pairing code (from the other PC: Settings → This PC)</span>
            <input className={`${input} font-mono text-[12px]`} placeholder="claude-fleet://192.168.1.20:47821/…" value={code} onChange={(e) => setCode(e.target.value)} />
          </label>
        )}
        <div className="grid grid-cols-[1fr_1.2fr] gap-3">
          <label className="flex flex-col gap-1.5">
            <span className="text-[12px] text-white/60">Name</span>
            <input className={input} placeholder="Laptop" value={name} onChange={(e) => setName(e.target.value)} />
          </label>
          <label className="flex flex-col gap-1.5">
            <span className="text-[12px] text-white/60">Address</span>
            <input
              className={input}
              placeholder="192.168.1.20 or laptop.tailnet.ts.net"
              value={address}
              onChange={(e) => {
                setAddress(e.target.value);
                setTest(null);
              }}
            />
          </label>
        </div>
        <label className="flex flex-col gap-1.5">
          <span className="text-[12px] text-white/60">Pairing token</span>
          <input
            className={`${input} font-mono text-[12px]`}
            type="password"
            placeholder="64 characters"
            value={token}
            onChange={(e) => {
              setToken(e.target.value);
              setTest(null);
            }}
          />
        </label>

        {duplicate && <div className="text-[12px] text-amber-200/90">{duplicate.name} already uses this address.</div>}
        {test && (
          <div className={`rounded-xl px-3 py-2 text-[12.5px] ${test.ok ? 'bg-emerald-500/10 text-emerald-100' : 'bg-rose-500/10 text-rose-100'}`}>
            {test.ok ? `Connected to ${test.info.machineName} · ${test.info.os} · collector v${test.info.collectorVersion}` : test.error}
          </div>
        )}

        <div className="flex justify-end gap-2">
          <button className={quiet} onClick={onCancel}>
            Cancel
          </button>
          <button className={quiet} disabled={!canTest || busy} onClick={run}>
            {busy ? 'Testing…' : 'Test connection'}
          </button>
          <button className={primary} disabled={!canTest || busy || !!duplicate} onClick={save}>
            {initial ? 'Save' : 'Add machine'}
          </button>
        </div>
      </div>
    </motion.div>
  );
}

export function MachinesSettings({
  saved,
  states,
  onChange,
}: {
  saved: MachineConfig[];
  states: Record<string, CollectorState>;
  onChange: (list: MachineConfig[]) => Promise<void>;
}) {
  const [adding, setAdding] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<string | null>(null);

  return (
    <div className="flex flex-col gap-2">
      {saved.length === 0 && !adding && (
        <div className="text-[12.5px] text-white/50">No other machines yet. Add your laptop with its pairing code.</div>
      )}
      {saved.map((m) => (
        <div key={m.id} className="flex flex-col gap-2">
          <div className="glass-tile flex items-center gap-3 rounded-2xl px-4 py-2.5">
            <div className="min-w-0 flex-1">
              <div className="flex items-center gap-2">
                <span className="truncate text-[13.5px] font-medium">{m.name}</span>
                <StatusChip s={states[m.id]} />
              </div>
              <div className="truncate text-[11.5px] text-white/45 tabular">
                {m.address}
                {states[m.id]?.snapshot && ` · ${states[m.id]!.snapshot!.machine.os}`}
              </div>
            </div>
            {confirmRemove === m.id ? (
              <>
                <span className="text-[12px] text-white/60">Remove {m.name}?</span>
                <button className={`${btn} bg-rose-500/25 text-rose-50 ring-rose-300/40 hover:bg-rose-500/40`} onClick={() => onChange(saved.filter((x) => x.id !== m.id)).then(() => setConfirmRemove(null))}>
                  Remove
                </button>
                <button className={quiet} onClick={() => setConfirmRemove(null)}>
                  Keep
                </button>
              </>
            ) : (
              <>
                <button className={quiet} onClick={() => setEditing(editing === m.id ? null : m.id)}>
                  Edit
                </button>
                <button className={quiet} onClick={() => setConfirmRemove(m.id)}>
                  Remove
                </button>
              </>
            )}
          </div>
          <AnimatePresence>
            {editing === m.id && (
              <MachineForm
                initial={m}
                existing={saved}
                onCancel={() => setEditing(null)}
                onSave={async (u) => {
                  await onChange(saved.map((x) => (x.id === u.id ? u : x)));
                  setEditing(null);
                }}
              />
            )}
          </AnimatePresence>
        </div>
      ))}
      <AnimatePresence>
        {adding && (
          <MachineForm
            existing={saved}
            onCancel={() => setAdding(false)}
            onSave={async (m) => {
              await onChange([...saved, m]);
              setAdding(false);
            }}
          />
        )}
      </AnimatePresence>
      {!adding && (
        <div>
          <button className={primary} onClick={() => setAdding(true)}>
            + Add machine
          </button>
        </div>
      )}
    </div>
  );
}
