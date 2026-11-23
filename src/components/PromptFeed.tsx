import { AnimatePresence, motion } from 'framer-motion';
import { useState } from 'react';
import type { ClaudeSummary, PromptEntry, TokenTotals } from '../types';
import { ago, dayTime, modelLabel, tokens } from '../lib/format';
import { Chip, effortTone, modelTone } from './Chip';
import { WorkingNow } from './WorkingNow';

function Stat({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="min-w-0">
      <div className="text-[10px] font-medium uppercase tracking-[0.14em] text-white/40">{label}</div>
      <div className="font-display text-lg font-semibold leading-tight tabular">{value}</div>
      {sub && <div className="truncate text-[11px] text-white/45 tabular">{sub}</div>}
    </div>
  );
}

function totalOf(t: TokenTotals) {
  return t.input + t.output + t.cacheWrite + t.cacheRead;
}

function PromptRow({ p, now }: { p: PromptEntry; now: number }) {
  const m = modelLabel(p.model);
  const t = p.tokens;
  const [open, setOpen] = useState(false);
  const long = p.text.length > 160 || p.text.split('\n').length > 2;
  return (
    <motion.li
      layout
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0 }}
      transition={{ duration: 0.25 }}
      className="glass-tile rounded-2xl px-3.5 py-2.5"
    >
      <div className="mb-1 flex items-center gap-2 text-[11px] text-white/45 tabular">
        {p.status === 'working' && <span className="live-dot h-1.5 w-1.5 rounded-full bg-emerald-400" title="Claude is working on this" />}
        <span title={new Date(p.ts).toLocaleString()}>{now - p.ts < 3600_000 ? ago(p.ts, now) : dayTime(p.ts)}</span>
        {p.project && <span className="truncate text-white/35">· {p.project}</span>}
        {p.status === 'working' && <span className="ml-auto text-emerald-300/90">working</span>}
        {p.status === 'stopped' && (
          <span className="ml-auto text-white/35" title="Claude stopped before finishing (interrupted, or the session was closed)">
            stopped
          </span>
        )}
      </div>
      <div
        onClick={long ? () => setOpen((o) => !o) : undefined}
        className={`select-text text-[13px] leading-snug ${open ? 'thin-scroll max-h-[50vh] overflow-y-auto whitespace-pre-wrap' : 'line-clamp-3'} ${
          p.kind === 'prompt' ? 'text-white/90' : 'font-mono text-[12px] text-white/70'
        } ${long ? 'cursor-pointer' : ''}`}
        title={long && !open ? 'Click to show the whole prompt' : undefined}
      >
        {p.text}
      </div>
      {long && (
        <button onClick={() => setOpen((o) => !o)} className="mt-1 text-[11.5px] text-violet-200/80 hover:text-violet-100">
          {open ? 'Show less' : 'Show more'}
        </button>
      )}
      {p.kind === 'prompt' || t.requests > 0 ? (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {p.model && <Chip tone={modelTone(m.family)}>{m.label}</Chip>}
          {p.effort && (
            <Chip tone={effortTone(p.effort)} title={p.effortIsDefault ? 'Default from settings.json' : 'Effort used for this turn'}>
              {p.effort}
              {p.effortIsDefault ? '*' : ''}
            </Chip>
          )}
          <Chip title="Uncached input tokens">in {tokens(t.input)}</Chip>
          <Chip title={`Output tokens (thinking ${t.thinking.toLocaleString()})`}>out {tokens(t.output)}</Chip>
          <Chip tone="muted" title="Cache writes / cache reads">
            cache {tokens(t.cacheWrite)} / {tokens(t.cacheRead)}
          </Chip>
          {t.requests > 1 && <Chip tone="muted" title="API responses in this turn">{t.requests} calls</Chip>}
          {p.subagentTokens > 0 && (
            <Chip tone="muted" title="Tokens used by subagents, included in the totals">
              agents {tokens(p.subagentTokens)}
            </Chip>
          )}
        </div>
      ) : null}
    </motion.li>
  );
}

export function PromptFeed({ claude, now, machine }: { claude: ClaudeSummary; now: number; machine: string }) {
  const { today, week } = claude;
  const workingIds = new Set(claude.working.map((p) => p.id));
  return (
    <div className="flex min-h-0 flex-col">
      <div className="mb-3 grid grid-cols-4 gap-3">
        <Stat label="Prompts today" value={today.prompts.toLocaleString()} sub={`${week.prompts.toLocaleString()} this week`} />
        <Stat label="Output today" value={tokens(today.output)} sub={`${tokens(week.output)} this week`} />
        <Stat label="Tokens today" value={tokens(totalOf(today))} sub={`${tokens(totalOf(week))} this week`} />
        <Stat
          label="Models today"
          value={claude.todayByModel.length ? `${claude.todayByModel.length}` : '–'}
          sub={claude.todayByModel.map((x) => modelLabel(x.model).label).join(', ') || 'no activity'}
        />
      </div>
      {claude.working.length > 0 && (
        <div className="mb-3">
          <div className="mb-1.5 text-[10px] font-medium uppercase tracking-[0.14em] text-emerald-300/80">Working now</div>
          <WorkingNow items={claude.working.map((p) => ({ machine, machineId: machine, prompt: p }))} now={now} showMachine={false} />
        </div>
      )}
      <ul className="thin-scroll -mr-2 flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto pr-2">
        <AnimatePresence initial={false}>
          {claude.recent.filter((p) => !workingIds.has(p.id)).map((p) => (
            <PromptRow key={p.id} p={p} now={now} />
          ))}
        </AnimatePresence>
        {claude.recent.length === 0 && (
          <li className="py-8 text-center text-[13px] text-white/40">No Claude Code prompts found on this machine yet.</li>
        )}
      </ul>
    </div>
  );
}
