import type { ReactNode } from 'react';

const TONES = {
  opus: 'bg-violet-500/15 text-violet-200 border-violet-400/25',
  sonnet: 'bg-sky-500/15 text-sky-200 border-sky-400/25',
  haiku: 'bg-emerald-500/15 text-emerald-200 border-emerald-400/25',
  fable: 'bg-amber-500/15 text-amber-200 border-amber-400/25',
  neutral: 'bg-white/[0.06] text-white/70 border-white/10',
  muted: 'bg-white/[0.03] text-white/45 border-white/[0.07]',
  high: 'bg-rose-500/12 text-rose-200 border-rose-400/20',
  medium: 'bg-indigo-500/12 text-indigo-200 border-indigo-400/20',
  low: 'bg-slate-500/15 text-slate-200 border-slate-400/20',
  live: 'bg-emerald-500/15 text-emerald-200 border-emerald-400/30',
  warn: 'bg-amber-500/15 text-amber-200 border-amber-400/30',
  bad: 'bg-rose-500/15 text-rose-200 border-rose-400/30',
} as const;

export type Tone = keyof typeof TONES;

export function Chip({ tone = 'neutral', children, title }: { tone?: Tone; children: ReactNode; title?: string }) {
  return (
    <span
      title={title}
      className={`inline-flex items-center gap-1 whitespace-nowrap rounded-full border px-2 py-[1px] text-[11px] font-medium leading-[18px] tabular ${TONES[tone]}`}
    >
      {children}
    </span>
  );
}

export function effortTone(effort: string | null): Tone {
  switch (effort) {
    case 'max':
    case 'xhigh':
    case 'high':
      return 'high';
    case 'medium':
      return 'medium';
    case 'low':
      return 'low';
    default:
      return 'muted';
  }
}

export function modelTone(family: string): Tone {
  return family in TONES ? (family as Tone) : 'neutral';
}
