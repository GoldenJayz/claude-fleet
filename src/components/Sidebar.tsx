import { motion } from 'framer-motion';
import type { ReactNode } from 'react';
import type { CollectorState } from '../lib/collector';
import type { MachineConfig } from '../types';

export type View = { kind: 'overview' } | { kind: 'machine'; id: string } | { kind: 'settings' };

function Item({
  active,
  collapsed,
  icon,
  label,
  sub,
  badge,
  onClick,
}: {
  active: boolean;
  collapsed: boolean;
  icon: ReactNode;
  label: string;
  sub?: ReactNode;
  badge?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      title={collapsed ? label : undefined}
      className={`group relative flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left transition-colors ${
        active ? 'bg-white/[0.11] text-white' : 'text-white/70 hover:bg-white/[0.06] hover:text-white'
      }`}
    >
      {active && <motion.span layoutId="nav-active" className="absolute left-0 top-2 bottom-2 w-[3px] rounded-full bg-gradient-to-b from-violet-300 to-sky-400" />}
      <span className="flex h-7 w-7 shrink-0 items-center justify-center">{icon}</span>
      {!collapsed && (
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[13px] font-medium">{label}</span>
          {sub && <span className="block truncate text-[11px] text-white/45 tabular">{sub}</span>}
        </span>
      )}
      {badge && <span className={collapsed ? 'absolute right-1 top-1' : 'shrink-0'}>{badge}</span>}
    </button>
  );
}

function statusColor(s?: CollectorState) {
  switch (s?.status) {
    case 'online':
      return 'bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.8)]';
    case 'connecting':
      return 'bg-white/40';
    default:
      return 'bg-rose-400 shadow-[0_0_8px_rgba(251,113,133,0.7)]';
  }
}

const icons = {
  overview: (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <rect x="3" y="3" width="7.5" height="7.5" rx="2" />
      <rect x="13.5" y="3" width="7.5" height="7.5" rx="2" />
      <rect x="3" y="13.5" width="7.5" height="7.5" rx="2" />
      <rect x="13.5" y="13.5" width="7.5" height="7.5" rx="2" />
    </svg>
  ),
  settings: (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
    </svg>
  ),
  pc: (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <rect x="2.5" y="4" width="19" height="12.5" rx="2" />
      <path d="M8 20.5h8M12 16.5v4" />
    </svg>
  ),
};

export function Sidebar({
  view,
  onView,
  collapsed,
  onToggle,
  machines,
  states,
}: {
  view: View;
  onView: (v: View) => void;
  collapsed: boolean;
  onToggle: () => void;
  machines: MachineConfig[];
  states: Record<string, CollectorState>;
}) {
  return (
    <motion.nav
      animate={{ width: collapsed ? 64 : 236 }}
      transition={{ type: 'spring', stiffness: 260, damping: 30 }}
      className="glass ml-3 mb-3 flex shrink-0 flex-col overflow-hidden rounded-3xl p-2"
    >
      <button
        onClick={onToggle}
        title={collapsed ? 'Expand sidebar' : 'Collapse sidebar'}
        className="mb-2 flex h-9 items-center gap-3 rounded-xl px-2.5 text-white/55 hover:bg-white/[0.06] hover:text-white"
      >
        <span className="flex h-7 w-7 shrink-0 items-center justify-center">
          <motion.svg animate={{ rotate: collapsed ? 180 : 0 }} width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
            <path d="M15 6l-6 6 6 6" />
          </motion.svg>
        </span>
        {!collapsed && <span className="text-[12px]">Collapse</span>}
      </button>

      <Item active={view.kind === 'overview'} collapsed={collapsed} icon={icons.overview} label="Overview" onClick={() => onView({ kind: 'overview' })} />

      {!collapsed && <div className="mt-4 mb-1 px-3 text-[10px] font-medium uppercase tracking-[0.16em] text-white/35">Machines</div>}
      {collapsed && <div className="mx-3 my-3 h-px bg-white/10" />}
      <div className="thin-scroll flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto">
        {machines.map((m) => {
          const st = states[m.id];
          const met = st?.snapshot?.metrics;
          const working = st?.status === 'online' ? (st.snapshot?.claude.working.length ?? 0) : 0;
          return (
            <Item
              key={m.id}
              active={view.kind === 'machine' && view.id === m.id}
              collapsed={collapsed}
              icon={
                <span className="relative">
                  {icons.pc}
                  <span className={`absolute -right-1 -bottom-0.5 h-2 w-2 rounded-full ring-2 ring-black/40 ${statusColor(st)}`} />
                </span>
              }
              label={st?.snapshot?.machine.machineName ?? m.name}
              sub={
                st?.status === 'online' && met
                  ? `CPU ${Math.round(met.cpu)}% · RAM ${Math.round((met.memUsed * 100) / met.memTotal)}%`
                  : st?.status === 'unauthorized'
                    ? 'token rejected'
                    : st?.status === 'connecting'
                      ? 'connecting…'
                      : 'offline'
              }
              badge={
                working > 0 ? (
                  <span className="flex h-5 min-w-5 items-center justify-center rounded-full bg-violet-500/30 px-1.5 text-[10.5px] font-semibold text-violet-100 ring-1 ring-violet-300/40" title={`${working} prompt${working === 1 ? '' : 's'} in progress`}>
                    {working}
                  </span>
                ) : undefined
              }
              onClick={() => onView({ kind: 'machine', id: m.id })}
            />
          );
        })}
      </div>

      <div className="mt-2 border-t border-white/10 pt-2">
        <Item active={view.kind === 'settings'} collapsed={collapsed} icon={icons.settings} label="Settings" onClick={() => onView({ kind: 'settings' })} />
      </div>
    </motion.nav>
  );
}
