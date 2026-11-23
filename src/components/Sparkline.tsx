import { useId } from 'react';
import type { Palette } from './RingGauge';

interface Series {
  values: (number | null)[];
  palette: Palette;
}

interface Props {
  series: Series[];
  /** Fixed max (e.g. 100 for percentages); otherwise scales to the data. */
  max?: number;
  width?: number;
  height?: number;
  /** Total points the window holds; fewer points are right-aligned. */
  capacity?: number;
}

function smoothPath(pts: [number, number][]) {
  if (pts.length === 0) return '';
  if (pts.length === 1) return `M${pts[0][0]},${pts[0][1]}`;
  let d = `M${pts[0][0]},${pts[0][1]}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i - 1] ?? pts[i];
    const p1 = pts[i];
    const p2 = pts[i + 1];
    const p3 = pts[i + 2] ?? p2;
    const c1x = p1[0] + (p2[0] - p0[0]) / 6;
    const c1y = p1[1] + (p2[1] - p0[1]) / 6;
    const c2x = p2[0] - (p3[0] - p1[0]) / 6;
    const c2y = p2[1] - (p3[1] - p1[1]) / 6;
    d += ` C${c1x.toFixed(1)},${c1y.toFixed(1)} ${c2x.toFixed(1)},${c2y.toFixed(1)} ${p2[0].toFixed(1)},${p2[1].toFixed(1)}`;
  }
  return d;
}

/** 60-second trend line (30 samples at 2 s) with a soft gradient fill. */
export function Sparkline({ series, max, width = 160, height = 36, capacity = 30 }: Props) {
  const id = useId().replace(/:/g, '');
  const all = series.flatMap((s) => s.values.filter((v): v is number => v != null));
  const top = max ?? Math.max(1, ...all) * 1.15;
  const step = width / Math.max(1, capacity - 1);
  const pad = 2;

  return (
    <svg width="100%" height={height} viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" className="block overflow-visible">
      <defs>
        {series.map((s, i) => (
          <linearGradient key={i} id={`f${id}${i}`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={s.palette.to} stopOpacity={0.35} />
            <stop offset="100%" stopColor={s.palette.to} stopOpacity={0} />
          </linearGradient>
        ))}
      </defs>
      {series.map((s, i) => {
        const vals = s.values.slice(-capacity);
        const offset = capacity - vals.length;
        const pts: [number, number][] = vals.map((v, j) => [
          (offset + j) * step,
          pad + (height - 2 * pad) * (1 - Math.min(1, (v ?? 0) / top)),
        ]);
        if (pts.length < 2) return null;
        const line = smoothPath(pts);
        const area = `${line} L${pts[pts.length - 1][0]},${height} L${pts[0][0]},${height} Z`;
        return (
          <g key={i}>
            <path d={area} fill={`url(#f${id}${i})`} />
            <path
              d={line}
              fill="none"
              stroke={s.palette.from}
              strokeWidth={1.6}
              strokeLinecap="round"
              vectorEffect="non-scaling-stroke"
              style={{ filter: `drop-shadow(0 0 3px ${s.palette.glow})` }}
            />
          </g>
        );
      })}
    </svg>
  );
}
