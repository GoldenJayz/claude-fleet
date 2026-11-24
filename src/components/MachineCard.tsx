import { motion } from 'framer-motion';
import { useRef, type ReactNode } from 'react';
import type { CollectorState } from '../lib/collector';
import { ago, bytes, duration, rate } from '../lib/format';
import { isMobile } from '../lib/machines';
import type { MachineConfig, MetricsSample, Snapshot } from '../types';
import { Chip } from './Chip';
import { ProcessList } from './ProcessList';
import { PromptFeed } from './PromptFeed';
import { PALETTES, RingGauge, type Palette } from './RingGauge';
import { Sparkline } from './Sparkline';

function systemDisk(m: MetricsSample) {
  return m.disks.find((d) => d.mount.toUpperCase().startsWith('C:')) ?? m.disks[0];
}

function primaryGpu(m: MetricsSample) {
  return [...m.gpus].sort((a, b) => (b.memoryTotal ?? 0) - (a.memoryTotal ?? 0))[0];
}

function GaugeTile({
  label,
  value,
  palette,
  lines,
  spark,
  sparkMax,
  dimmed,
}: {
  label: string;
  value: number | null;
  palette: Palette;
  lines: ReactNode[];
  spark: { values: (number | null)[]; palette: Palette }[];
  sparkMax?: number;
  dimmed?: boolean;
}) {
  return (
    <div className="glass-tile flex min-w-0 flex-col gap-3 rounded-2xl p-3.5">
      <div className="flex items-center gap-3.5">
        <RingGauge value={value} palette={palette} label={label} dimmed={dimmed} size={isMobile ? 88 : undefined} stroke={isMobile ? 8 : undefined} />
        <div className="min-w-0 space-y-1 text-[12px] leading-snug text-white/60 tabular">
          {lines.map((l, i) => (
            <div key={i} className="truncate">
              {l}
            </div>
          ))}
        </div>
      </div>
      <Sparkline series={spark} max={sparkMax} />
    </div>
  );
}

function Panel({ title, right, children, className = '' }: { title: string; right?: ReactNode; children: ReactNode; className?: string }) {
  return (
    <section className={`glass-tile flex min-h-0 flex-col rounded-2xl p-4 ${className}`}>
      <div className="mb-3 flex items-center justify-between gap-3">
        <h3 className="font-display text-[14px] font-semibold text-white/90">{title}</h3>
        {right}
      </div>
      {children}
    </section>
  );
}

function StatusPill({ state, now }: { state: CollectorState; now: number }) {
  switch (state.status) {
    case 'online':
      return (
        <Chip tone="live">
          <span className="live-dot h-1.5 w-1.5 rounded-full bg-emerald-400" /> Live
        </Chip>
      );
    case 'connecting':
      return <Chip tone="muted">Connecting…</Chip>;
    case 'unauthorized':
      return <Chip tone="bad">Token rejected</Chip>;
    default:
      return (
        <Chip tone="bad" title={state.error ?? undefined}>
          Offline{state.lastMessageAt ? ` · ${ago(state.lastMessageAt, now)}` : ''}
        </Chip>
      );
  }
}

function Body({
  snap,
  offline,
  now,
  machine,
  requestedBy,
}: {
  snap: Snapshot;
  offline: boolean;
  now: number;
  machine: MachineConfig;
  requestedBy: string;
}) {
  const m = snap.metrics;
  const h = snap.history;
  const disk = m ? systemDisk(m) : undefined;
  const gpu = m ? primaryGpu(m) : undefined;
  const diskUsed = disk ? ((disk.total - disk.available) * 100) / disk.total : null;
  const freeAll = m?.disks.reduce((a, d) => a + d.available, 0) ?? 0;

  return (
    <div className={`flex flex-col gap-4 transition-opacity ${offline ? 'opacity-55 saturate-50' : ''}`}>
      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 2xl:grid-cols-4">
        <GaugeTile
          label="CPU"
          value={m?.cpu ?? null}
          palette={PALETTES.cpu}
          lines={[
            <span className="text-white/85">{m ? `${(m.cpuFreqMhz / 1000).toFixed(2)} GHz` : '–'}</span>,
            `${snap.machine.logicalCores} threads`,
            m?.temps.find((t) => /cpu|package|core/i.test(t.label)) ? `${Math.round(m.temps.find((t) => /cpu|package|core/i.test(t.label))!.celsius)}°C` : 'temp n/a',
          ]}
          spark={[{ values: h.map((p) => p.cpu), palette: PALETTES.cpu }]}
          sparkMax={100}
        />
        <GaugeTile
          label="RAM"
          value={m ? (m.memUsed * 100) / m.memTotal : null}
          palette={PALETTES.ram}
          lines={[
            <span className="text-white/85">{m ? `${bytes(m.memUsed)} used` : '–'}</span>,
            m ? `of ${bytes(m.memTotal, 0)}` : '',
            m && m.swapTotal ? `swap ${bytes(m.swapUsed, 0)}` : '',
          ]}
          spark={[{ values: h.map((p) => p.memPct), palette: PALETTES.ram }]}
          sparkMax={100}
        />
        <GaugeTile
          label={disk ? `Disk ${disk.mount.replace('\\', '')}` : 'Disk'}
          value={diskUsed}
          palette={PALETTES.disk}
          lines={[
            <span className="text-white/85">{disk ? `${bytes(disk.available, 0)} free` : '–'}</span>,
            m ? `R ${rate(m.diskReadBps)}` : '',
            m ? `W ${rate(m.diskWriteBps)}` : '',
          ]}
          spark={[
            { values: h.map((p) => p.diskReadBps), palette: PALETTES.disk },
            { values: h.map((p) => p.diskWriteBps), palette: { ...PALETTES.disk, from: '#fde68a' } },
          ]}
        />
        <GaugeTile
          label="GPU"
          value={gpu?.usage ?? null}
          palette={PALETTES.gpu}
          lines={[
            <span className="text-white/85" title={gpu?.name}>{gpu ? gpu.name.replace(/\(R\)|\(TM\)/g, '') : 'No GPU'}</span>,
            gpu?.memoryTotal ? `VRAM ${bytes(gpu.memoryUsed ?? 0)} / ${bytes(gpu.memoryTotal, 0)}` : '',
            gpu?.temperatureC != null ? `${Math.round(gpu.temperatureC)}°C` : 'temp n/a',
          ]}
          spark={[{ values: h.map((p) => p.gpu), palette: PALETTES.gpu }]}
          sparkMax={100}
        />
      </div>

      <div className="glass-tile grid grid-cols-[auto_1fr] items-center gap-x-5 gap-y-2 rounded-2xl px-4 py-2.5 md:grid-cols-[auto_1fr_auto]">
        <div className="flex gap-5 text-[12.5px] tabular">
          <span>
            <span className="text-sky-300">↓</span> <span className="text-white/85">{rate(m?.netRxBps)}</span>
          </span>
          <span>
            <span className="text-violet-300">↑</span> <span className="text-white/85">{rate(m?.netTxBps)}</span>
          </span>
        </div>
        <Sparkline
          height={26}
          series={[
            { values: h.map((p) => p.netRxBps), palette: { from: '#7dd3fc', to: '#38bdf8', glow: 'rgba(56,189,248,0.4)' } },
            { values: h.map((p) => p.netTxBps), palette: { from: '#c4b5fd', to: '#a78bfa', glow: 'rgba(167,139,250,0.4)' } },
          ]}
        />
        <div className="col-span-2 flex flex-wrap gap-1.5 md:col-span-1 md:justify-end">
          {m?.disks.length ? <Chip tone="muted">{m.disks.length} drives · {bytes(freeAll, 0)} free</Chip> : null}
          {m?.temps.slice(0, 3).map((t) => (
            <Chip key={t.label} tone="muted">
              {t.label} {Math.round(t.celsius)}°C
            </Chip>
          ))}
        </div>
      </div>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1.55fr)_minmax(0,1fr)]">
        <Panel
          title="Claude Code"
          className="h-[clamp(420px,68vh,820px)]"
          right={
            snap.claude.lastActivityTs ? (
              <span className="text-[11px] text-white/40">last reply {ago(snap.claude.lastActivityTs, now)}</span>
            ) : null
          }
        >
          <PromptFeed claude={snap.claude} now={now} machine={snap.machine.machineName} />
        </Panel>
        <Panel
          title="Processes"
          className="h-[clamp(420px,68vh,820px)]"
          right={snap.heavy.length ? <Chip tone="warn">{snap.heavy.length} heavy</Chip> : <Chip tone="muted">by CPU · RAM</Chip>}
        >
          <ProcessList
            machine={machine}
            machineName={snap.machine.machineName}
            processes={snap.processes}
            heavy={snap.heavy ?? []}
            rules={snap.heavyRules}
            totalMemory={m?.memTotal ?? snap.machine.totalMemory}
            requestedBy={requestedBy}
            now={now}
            offline={offline}
          />
        </Panel>
      </div>
    </div>
  );
}

export function MachineCard({
  machine,
  state,
  now,
  requestedBy,
  onSendFiles,
  onOpenTerminal,
  onOpenScreen,
}: {
  machine: MachineConfig;
  state: CollectorState;
  now: number;
  /** Name of the machine this dashboard runs on, recorded in the kill log. */
  requestedBy: string;
  onSendFiles?: (files: File[]) => void;
  onOpenTerminal?: () => void;
  onOpenScreen?: () => void;
}) {
  const snap = state.snapshot;
  const info = snap?.machine;
  const offline = state.status !== 'online';
  const picker = useRef<HTMLInputElement>(null);
  const canFiles = !machine.isLocal && !offline && !!info?.features?.includes('files');
  const action =
    'flex items-center gap-1.5 rounded-xl bg-white/[0.06] px-3 py-1.5 text-[12.5px] font-medium text-white/85 ring-1 ring-white/10 transition-colors hover:bg-white/[0.12] disabled:opacity-35';

  return (
    <motion.article
      layout
      initial={{ opacity: 0, y: 12, scale: 0.99 }}
      animate={{ opacity: 1, y: 0, scale: 1 }}
      transition={{ type: 'spring', stiffness: 120, damping: 20 }}
      className="glass flex min-w-0 shrink-0 flex-col rounded-[28px] p-3.5 md:p-5"
    >
      <header className="mb-4 flex flex-col gap-3 md:flex-row md:items-start md:justify-between md:gap-4">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
            <h2 className="font-display text-2xl font-semibold tracking-tight">{info?.machineName ?? machine.name}</h2>
            <StatusPill state={state} now={now} />
            {machine.isLocal && <Chip tone="muted">this PC</Chip>}
          </div>
          <div className="mt-1 text-[12.5px] text-white/45 md:truncate">
            {info
              ? `${info.os} · ${info.cpuBrand.replace(/\(R\)|\(TM\)|CPU /g, '').replace(/\s+/g, ' ')} · ${bytes(info.totalMemory, 0)} RAM · up ${duration(now - info.bootTimeMs)}`
              : machine.address}
          </div>
        </div>
        <div className="flex items-start gap-4 md:shrink-0">
          {!machine.isLocal && (
            <div className="flex items-center gap-2">
              <input
                ref={picker}
                type="file"
                multiple
                className="hidden"
                onChange={(e) => {
                  const files = Array.from(e.target.files ?? []);
                  if (files.length) onSendFiles?.(files);
                  e.target.value = '';
                }}
              />
              <button
                className={action}
                disabled={!canFiles}
                title={canFiles ? `Send files to ${info?.machineName ?? machine.name} (or drag them onto the window)` : offline ? 'Offline' : 'Update Claude Fleet on that PC first'}
                onClick={() => picker.current?.click()}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M12 16V4M7 9l5-5 5 5M4 17v2a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2" />
                </svg>
                Send files
              </button>
              <button className={action} disabled={offline} title={`Open a PowerShell on ${info?.machineName ?? machine.name}`} onClick={onOpenTerminal}>
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                  <path d="M5 8l4 4-4 4M12 17h7" />
                </svg>
                Terminal
              </button>
              {info?.features?.includes('screen') && (
                <button className={action} disabled={offline} title={`See ${info?.machineName ?? machine.name}’s screen`} onClick={onOpenScreen}>
                  <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                    <rect x="3" y="4" width="18" height="12" rx="2" />
                    <path d="M8 20h8M12 16v4" />
                  </svg>
                  Screen
                </button>
              )}
            </div>
          )}
          <div className="hidden text-right text-[11px] text-white/35 tabular md:block">
            {machine.address}
            {info && <div>collector v{info.collectorVersion}</div>}
          </div>
        </div>
      </header>

      {snap ? (
        <Body snap={snap} offline={offline} now={now} machine={machine} requestedBy={requestedBy} />
      ) : (
        <div className="flex flex-1 items-center justify-center py-24 text-[13px] text-white/45">
          {state.status === 'connecting' ? 'Connecting…' : state.error ?? 'Offline'}
        </div>
      )}
    </motion.article>
  );
}
