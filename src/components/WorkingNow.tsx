import { AnimatePresence, motion } from 'framer-motion';
import type { Activity, PromptEntry } from '../types';
import { duration, modelLabel, tokens } from '../lib/format';
import { Chip, effortTone, modelTone } from './Chip';

/** "Editing App.tsx", "Running: Build the Tauri crate", "Thinking…" */
export function describeActivity(a: Activity | null): string {
  if (!a) return 'Starting…';
  if (a.kind === 'thinking') return 'Thinking…';
  if (a.kind === 'text') return 'Writing a reply…';
  const tool = a.tool ?? 'a tool';
  const d = a.detail;
  const verb: Record<string, string> = {
    Bash: 'Running',
    PowerShell: 'Running',
    Edit: 'Editing',
    MultiEdit: 'Editing',
    Write: 'Writing',
    Read: 'Reading',
    NotebookEdit: 'Editing',
    Grep: 'Searching for',
    Glob: 'Looking for files',
    WebSearch: 'Searching the web for',
    WebFetch: 'Reading',
    Agent: 'Delegating',
    Task: 'Delegating',
    ToolSearch: 'Loading tools',
  };
  if (verb[tool]) return d ? `${verb[tool]} ${d}` : verb[tool];
  if (tool.startsWith('mcp__')) {
    const parts = tool.split('__');
    return `Using ${parts[parts.length - 1].replace(/_/g, ' ')}`;
  }
  return d ? `${tool}: ${d}` : `Using ${tool}`;
}

export interface WorkingItem {
  machine: string;
  machineId: string;
  prompt: PromptEntry;
}

function Row({ item, now, showMachine, onOpen }: { item: WorkingItem; now: number; showMachine: boolean; onOpen?: (id: string) => void }) {
  const p = item.prompt;
  const m = modelLabel(p.model);
  const step = describeActivity(p.activity);
  return (
    <motion.li
      layout
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0, marginTop: 0 }}
      transition={{ duration: 0.25 }}
      onClick={onOpen ? () => onOpen(item.machineId) : undefined}
      className={`working-sweep relative overflow-hidden rounded-2xl border border-violet-300/20 bg-violet-500/[0.07] px-4 py-3 ${onOpen ? 'cursor-pointer hover:border-violet-300/35' : ''}`}
    >
      <div className="flex items-center gap-2 text-[11.5px] text-white/55 tabular">
        <span className="live-dot h-2 w-2 shrink-0 rounded-full bg-emerald-400" />
        {showMachine && <span className="font-medium text-white/80">{item.machine}</span>}
        {p.project && <span className="truncate">{showMachine ? '· ' : ''}{p.project}</span>}
        <span className="ml-auto shrink-0">working for {duration(now - p.ts)}</span>
      </div>
      <div className="mt-1.5 line-clamp-2 select-text text-[14px] leading-snug text-white/95">{p.text}</div>
      <div className="mt-2 flex items-center gap-2 text-[12.5px] text-violet-100/90">
        <svg width="14" height="14" viewBox="0 0 24 24" className="shrink-0 text-violet-300">
          <circle cx="12" cy="12" r="9" fill="none" stroke="currentColor" strokeOpacity="0.25" strokeWidth="3" />
          <path d="M21 12a9 9 0 0 0-9-9" fill="none" stroke="currentColor" strokeWidth="3" strokeLinecap="round" />
        </svg>
        <span className="truncate">{step}</span>
        {p.activity?.inSubagent && <Chip tone="muted">subagent</Chip>}
        {p.activity && <span className="shrink-0 text-[11px] text-white/35">{duration(now - p.activity.ts)} ago</span>}
      </div>
      <div className="mt-2 flex flex-wrap gap-1.5">
        {p.model && <Chip tone={modelTone(m.family)}>{m.label}</Chip>}
        {p.effort && <Chip tone={effortTone(p.effort)}>{p.effort}</Chip>}
        <Chip title="Output tokens so far">out {tokens(p.tokens.output)}</Chip>
        <Chip tone="muted" title="API calls so far">{p.tokens.requests} calls</Chip>
        {p.subagentTokens > 0 && <Chip tone="muted">agents {tokens(p.subagentTokens)}</Chip>}
      </div>
    </motion.li>
  );
}

export function WorkingNow({
  items,
  now,
  showMachine = true,
  onOpen,
  emptyText = 'Claude isn’t working on anything right now.',
}: {
  items: WorkingItem[];
  now: number;
  showMachine?: boolean;
  onOpen?: (id: string) => void;
  emptyText?: string;
}) {
  return (
    <ul className="flex flex-col gap-2">
      <AnimatePresence initial={false}>
        {items.map((it) => (
          <Row key={`${it.machineId}:${it.prompt.id}`} item={it} now={now} showMachine={showMachine} onOpen={onOpen} />
        ))}
      </AnimatePresence>
      {items.length === 0 && <li className="rounded-2xl px-1 py-2 text-[13px] text-white/45">{emptyText}</li>}
    </ul>
  );
}
