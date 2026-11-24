import { motion } from 'framer-motion';
import { ago, clockTime, duration } from '../lib/format';
import type { PlanView, WindowView } from '../lib/plan';
import { Chip } from './Chip';

function barColors(used: number) {
  if (used >= 95) return 'from-rose-400 to-rose-600 shadow-[0_0_18px_rgba(244,63,94,0.55)]';
  if (used >= 80) return 'from-amber-300 to-orange-500 shadow-[0_0_18px_rgba(251,146,60,0.5)]';
  return 'from-[#f0a47a] via-[#c084fc] to-[#60a5fa] shadow-[0_0_18px_rgba(192,132,252,0.45)]';
}

function when(ts: number, now: number) {
  return new Date(ts).toDateString() === new Date(now).toDateString()
    ? clockTime(ts)
    : new Date(ts).toLocaleString([], { weekday: 'short', hour: 'numeric', minute: '2-digit' });
}

function WindowBar({ w, now }: { w: WindowView; now: number }) {
  const used = w.used ?? 0;
  return (
    <div className="min-w-0 flex-1">
      <div className="mb-1.5 flex items-baseline justify-between gap-3">
        <span className="text-[12px] font-medium uppercase tracking-[0.12em] text-white/55">{w.label}</span>
        <span className="flex items-baseline gap-2 tabular">
          <span className="font-display text-xl font-semibold" title={w.atLeast ? 'From an older reading; usage can only have gone up since' : undefined}>
            {w.used == null ? '–' : `${w.atLeast ? '≥ ' : ''}${Math.round(used)}%`}
          </span>
          <span className="text-[12px] text-white/50">
            {w.resetSinceReading
              ? 'new window'
              : w.used == null
                ? 'unknown'
                : `${w.atLeast ? '≤ ' : ''}${Math.max(0, 100 - Math.round(used))}% left`}
          </span>
        </span>
      </div>
      <div className="relative h-2.5 overflow-hidden rounded-full bg-white/[0.07]">
        <motion.div
          className={`absolute inset-y-0 left-0 rounded-full bg-gradient-to-r ${barColors(used)} ${w.atLeast ? 'opacity-60' : ''}`}
          initial={{ width: 0 }}
          animate={{ width: `${Math.min(100, used)}%` }}
          transition={{ duration: 0.6, ease: 'easeOut' }}
        />
        {/* 80% marker */}
        <div className="absolute inset-y-0 w-px bg-white/25" style={{ left: '80%' }} />
      </div>
      <div className="mt-1.5 text-[12px] text-white/55 tabular">
        {w.resetSinceReading && w.resetsAt ? (
          <>
            Reset at <span className="text-white/85">{when(w.resetsAt, now)}</span>
            <span className="text-white/35"> · waiting for a new reading</span>
          </>
        ) : w.resetsAt ? (
          <>
            Resets in <span className="text-white/85">{duration(w.resetsAt - now)}</span>
            <span className="text-white/35"> · {when(w.resetsAt, now)}</span>
          </>
        ) : (
          'No reset time yet'
        )}
      </div>
    </div>
  );
}

export function PlanBanner({ plan, now }: { plan: PlanView; now: number }) {
  return (
    <motion.section initial={{ opacity: 0, y: -8 }} animate={{ opacity: 1, y: 0 }} className="glass shrink-0 rounded-3xl px-4 py-3.5 md:px-6 md:py-4">
      <div className="flex flex-col gap-3.5 md:flex-row md:items-center md:gap-8">
        <div className="md:w-48 md:shrink-0">
          <div className="font-display text-[15px] font-semibold glow-text">Plan usage</div>
          <div className="mt-1 flex flex-wrap gap-1.5">
            {plan.reading == null ? (
              <Chip tone="muted">no reading yet</Chip>
            ) : plan.fresh ? (
              <Chip tone="live">current</Chip>
            ) : (
              <Chip tone="warn" title="No PC with Claude Code has reported usage in the last 10 minutes">
                last reading
              </Chip>
            )}
            <Chip tone="muted" title="Shared by claude.ai, the apps and every machine on your account">whole account</Chip>
          </div>
          <div className="mt-1.5 text-[11px] leading-snug text-white/40">
            {plan.reading
              ? plan.reading.source === 'anthropic'
                ? `From Anthropic via ${plan.source ?? 'this PC'} · ${ago(plan.reading.capturedAtMs, now)}`
                : `From ${plan.source ?? 'a'} status line · ${ago(plan.reading.capturedAtMs, now)}`
              : 'Appears once a PC signed in to Claude Code is online'}
            {plan.reading && !plan.fresh && <div>Updates every 5 min while a PC signed in to Claude Code is online</div>}
          </div>
        </div>
        {plan.windows.map((w) => (
          <WindowBar key={w.key} w={w} now={now} />
        ))}
      </div>
    </motion.section>
  );
}
