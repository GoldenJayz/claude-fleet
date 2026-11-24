import { motion } from 'framer-motion';
import { useId } from 'react';

export interface Palette {
  from: string;
  to: string;
  glow: string;
}

export const PALETTES = {
  cpu: { from: '#22d3ee', to: '#3b82f6', glow: 'rgba(56,189,248,0.55)' },
  ram: { from: '#c084fc', to: '#e879f9', glow: 'rgba(217,70,239,0.5)' },
  disk: { from: '#fbbf24', to: '#f97316', glow: 'rgba(251,146,60,0.5)' },
  gpu: { from: '#34d399', to: '#14b8a6', glow: 'rgba(45,212,191,0.5)' },
  net: { from: '#60a5fa', to: '#a78bfa', glow: 'rgba(129,140,248,0.5)' },
} satisfies Record<string, Palette>;

interface Props {
  value: number | null; // 0–100
  palette: Palette;
  size?: number;
  stroke?: number;
  label: string;
  dimmed?: boolean;
}

/** Circular gauge with a gradient arc and soft glow; animates smoothly between values. */
export function RingGauge({ value, palette, size = 104, stroke = 9, label, dimmed }: Props) {
  const id = useId().replace(/:/g, '');
  const r = (size - stroke) / 2 - 4;
  const c = 2 * Math.PI * r;
  const v = value == null ? 0 : Math.max(0, Math.min(100, value));
  const hot = v >= 90;

  return (
    <div className="relative shrink-0" style={{ width: size, height: size, opacity: dimmed ? 0.45 : 1 }}>
      <svg width={size} height={size} viewBox={`0 0 ${size} ${size}`} className="-rotate-90">
        <defs>
          <linearGradient id={`g${id}`} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0%" stopColor={hot ? '#fb7185' : palette.from} />
            <stop offset="100%" stopColor={hot ? '#f43f5e' : palette.to} />
          </linearGradient>
        </defs>
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="rgba(255,255,255,0.08)" strokeWidth={stroke} />
        <motion.circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          stroke={`url(#g${id})`}
          strokeWidth={stroke}
          strokeLinecap="round"
          strokeDasharray={c}
          initial={{ strokeDashoffset: c }}
          animate={{ strokeDashoffset: c * (1 - v / 100) }}
          transition={{ duration: 0.4, ease: 'easeOut' }}
          style={{ filter: `drop-shadow(0 0 6px ${hot ? 'rgba(244,63,94,0.6)' : palette.glow})` }}
        />
      </svg>
      <div className="absolute inset-0 flex flex-col items-center justify-center">
        <span className="font-display text-[22px] font-semibold leading-none tabular">
          {value == null ? '–' : Math.round(v)}
          {value != null && <span className="ml-0.5 text-xs font-normal text-white/50">%</span>}
        </span>
        <span className="mt-1 text-[10px] font-medium uppercase tracking-[0.14em] text-white/45">{label}</span>
      </div>
    </div>
  );
}
