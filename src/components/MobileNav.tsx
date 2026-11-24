import type { ReactNode } from 'react';
import type { CollectorState } from '../lib/collector';
import type { MachineConfig } from '../types';
import { icons, type View } from './Sidebar';

/** Phone header: the app name and how many PCs are online. */
export function MobileHeader({ online, total }: { online: number; total: number }) {
  return (
    <header className="flex shrink-0 items-center gap-2.5 px-4 pb-2 pt-3">
      <svg width="20" height="20" viewBox="0 0 24 24">
        <defs>
          <linearGradient id="mh" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#f0a47a" />
            <stop offset="0.5" stopColor="#c084fc" />
            <stop offset="1" stopColor="#60a5fa" />
          </linearGradient>
        </defs>
        <circle cx="12" cy="12" r="8" fill="none" stroke="url(#mh)" strokeWidth="3" strokeDasharray="36 50" strokeLinecap="round" transform="rotate(-90 12 12)" />
      </svg>
      <span className="font-display text-[16px] font-semibold tracking-wide text-white/90">Claude Fleet</span>
      <span className="ml-auto text-[12px] text-white/45 tabular">
        {online}/{total} online
      </span>
    </header>
  );
}

function Tab({ active, icon, label, dot, onClick }: { active: boolean; icon: ReactNode; label: string; dot?: string; onClick: () => void }) {
  return (
    <button
      onClick={onClick}
      className={`relative flex min-w-[64px] flex-1 flex-col items-center gap-0.5 rounded-2xl px-2 py-1.5 transition-colors ${
        active ? 'bg-white/[0.11] text-white' : 'text-white/55 active:bg-white/[0.06]'
      }`}
    >
      <span className="relative flex h-6 items-center">
        {icon}
        {dot && <span className={`absolute -right-1.5 bottom-0 h-2 w-2 rounded-full ring-2 ring-black/50 ${dot}`} />}
      </span>
      <span className="max-w-[84px] truncate text-[10.5px] font-medium">{label}</span>
    </button>
  );
}

function dotFor(s?: CollectorState) {
  if (s?.status === 'online') return 'bg-emerald-400';
  if (s?.status === 'connecting' || !s) return 'bg-white/40';
  return 'bg-rose-400';
}

/** Phone tab bar: Overview, one tab per PC, open terminals, Settings. */
export function MobileNav({
  view,
  onView,
  machines,
  states,
  terminals,
  agentsBusy = 0,
}: {
  agentsBusy?: number;
  view: View;
  onView: (v: View) => void;
  machines: MachineConfig[];
  states: Record<string, CollectorState>;
  terminals: string[];
}) {
  const name = (m: MachineConfig) => states[m.id]?.snapshot?.machine.machineName ?? m.name;
  return (
    <nav className="glass mx-2 mb-2 flex shrink-0 gap-1 overflow-x-auto rounded-3xl p-1.5">
      <Tab active={view.kind === 'overview'} icon={icons.overview} label="Overview" onClick={() => onView({ kind: 'overview' })} />
      <Tab active={view.kind === 'agents'} icon={icons.agents} label="Agents" dot={agentsBusy ? 'bg-violet-300' : undefined} onClick={() => onView({ kind: 'agents' })} />
      {machines.map((m) => (
        <Tab
          key={m.id}
          active={view.kind === 'machine' && view.id === m.id}
          icon={icons.pc}
          dot={dotFor(states[m.id])}
          label={name(m)}
          onClick={() => onView({ kind: 'machine', id: m.id })}
        />
      ))}
      {terminals.map((id) => {
        const m = machines.find((x) => x.id === id);
        return m ? (
          <Tab key={`t-${id}`} active={view.kind === 'terminal' && view.id === id} icon={icons.terminal} label={name(m)} onClick={() => onView({ kind: 'terminal', id })} />
        ) : null;
      })}
      <Tab active={view.kind === 'settings'} icon={icons.settings} label="Settings" onClick={() => onView({ kind: 'settings' })} />
    </nav>
  );
}
