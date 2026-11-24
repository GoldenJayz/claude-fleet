import { motion } from 'framer-motion';
import type { CollectorState } from '../lib/collector';
import { ago, bytes, tokens } from '../lib/format';
import { isMobile } from '../lib/machines';
import type { MachineConfig } from '../types';
import { Chip } from './Chip';
import { PALETTES, RingGauge } from './RingGauge';
import { WorkingNow, type WorkingItem } from './WorkingNow';

function MachineTile({ machine, state, now, onOpen }: { machine: MachineConfig; state?: CollectorState; now: number; onOpen: () => void }) {
  const snap = state?.snapshot;
  const m = snap?.metrics;
  const online = state?.status === 'online';
  const disk = m?.disks.find((d) => d.mount.toUpperCase().startsWith('C:')) ?? m?.disks[0];
  const gpu = m ? [...m.gpus].sort((a, b) => (b.memoryTotal ?? 0) - (a.memoryTotal ?? 0))[0] : undefined;
  const working = online ? (snap?.claude.working.length ?? 0) : 0;
  const ring = isMobile ? 70 : 78;

  return (
    <motion.button
      layout
      onClick={onOpen}
      whileHover={{ y: -2 }}
      className="glass flex flex-col gap-4 rounded-3xl p-4 text-left md:p-5 transition-colors hover:border-white/20"
    >
      <div className="flex w-full items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2.5">
            <span className="truncate font-display text-xl font-semibold">{snap?.machine.machineName ?? machine.name}</span>
            {online ? (
              <Chip tone="live">
                <span className="live-dot h-1.5 w-1.5 rounded-full bg-emerald-400" /> Live
              </Chip>
            ) : state?.status === 'connecting' ? (
              <Chip tone="muted">Connecting…</Chip>
            ) : (
              <Chip tone="bad">{state?.status === 'unauthorized' ? 'Token rejected' : `Offline${state?.lastMessageAt ? ` · ${ago(state.lastMessageAt, now)}` : ''}`}</Chip>
            )}
            {machine.isLocal && <Chip tone="muted">this PC</Chip>}
          </div>
          <div className="mt-0.5 truncate text-[12px] text-white/45">{snap ? `${snap.machine.os} · ${bytes(snap.machine.totalMemory, 0)} RAM` : machine.address}</div>
        </div>
        {working > 0 && <Chip tone="medium">{working} working</Chip>}
      </div>

      <div className={`flex w-full items-center justify-between gap-2 ${online ? '' : 'opacity-50'}`}>
        <RingGauge size={ring} stroke={7} label="CPU" value={m?.cpu ?? null} palette={PALETTES.cpu} />
        <RingGauge size={ring} stroke={7} label="RAM" value={m ? (m.memUsed * 100) / m.memTotal : null} palette={PALETTES.ram} />
        <RingGauge size={ring} stroke={7} label="Disk" value={disk ? ((disk.total - disk.available) * 100) / disk.total : null} palette={PALETTES.disk} />
        <RingGauge size={ring} stroke={7} label="GPU" value={gpu?.usage ?? null} palette={PALETTES.gpu} />
      </div>

      <div className="grid w-full grid-cols-3 gap-3 border-t border-white/10 pt-3 text-[12px] tabular">
        <div>
          <div className="text-[10px] uppercase tracking-[0.14em] text-white/40">Prompts today</div>
          <div className="font-display text-base font-semibold">{snap ? snap.claude.today.prompts : '–'}</div>
        </div>
        <div>
          <div className="text-[10px] uppercase tracking-[0.14em] text-white/40">Output today</div>
          <div className="font-display text-base font-semibold">{snap ? tokens(snap.claude.today.output) : '–'}</div>
        </div>
        <div>
          <div className="text-[10px] uppercase tracking-[0.14em] text-white/40">Last reply</div>
          <div className="font-display text-base font-semibold">{snap?.claude.lastActivityTs ? ago(snap.claude.lastActivityTs, now) : '–'}</div>
        </div>
      </div>
    </motion.button>
  );
}

export function Overview({
  machines,
  states,
  now,
  onOpen,
}: {
  machines: MachineConfig[];
  states: Record<string, CollectorState>;
  now: number;
  onOpen: (id: string) => void;
}) {
  const working: WorkingItem[] = machines.flatMap((m) => {
    const st = states[m.id];
    if (st?.status !== 'online' || !st.snapshot) return [];
    return st.snapshot.claude.working.map((p) => ({ machine: st.snapshot!.machine.machineName, machineId: m.id, prompt: p }));
  });

  return (
    <div className="flex flex-col gap-3 md:gap-4">
      <section className="glass rounded-3xl p-4 md:p-5">
        <div className="mb-3 flex items-center justify-between">
          <h2 className="font-display text-[15px] font-semibold">Working now</h2>
          <span className="text-[12px] text-white/45">{working.length ? `${working.length} in progress` : 'idle'}</span>
        </div>
        <WorkingNow items={working} now={now} onOpen={onOpen} showMachine />
      </section>
      <div className="grid gap-4 lg:grid-cols-2 2xl:grid-cols-3">
        {machines.map((m) => (
          <MachineTile key={m.id} machine={m} state={states[m.id]} now={now} onOpen={() => onOpen(m.id)} />
        ))}
      </div>
    </div>
  );
}
