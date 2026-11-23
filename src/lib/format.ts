export function bytes(n: number | null | undefined, digits = 1): string {
  if (n == null || !isFinite(n)) return '–';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(i === 0 ? 0 : v >= 100 ? 0 : digits)} ${units[i]}`;
}

export function rate(bps: number | null | undefined): string {
  if (bps == null) return '–';
  return `${bytes(bps)}/s`;
}

export function tokens(n: number | null | undefined): string {
  if (n == null) return '–';
  if (n < 1000) return `${n}`;
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
  if (n < 1_000_000_000) return `${(n / 1_000_000).toFixed(n < 10_000_000 ? 2 : 1)}M`;
  return `${(n / 1_000_000_000).toFixed(2)}B`;
}

export function pct(n: number | null | undefined, digits = 0): string {
  if (n == null || !isFinite(n)) return '–';
  return `${n.toFixed(digits)}%`;
}

export function duration(ms: number): string {
  if (ms <= 0) return 'now';
  const s = Math.floor(ms / 1000);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m.toString().padStart(2, '0')}m`;
  if (m > 0) return `${m}m ${(s % 60).toString().padStart(2, '0')}s`;
  return `${s}s`;
}

export function ago(ts: number, now = Date.now()): string {
  const s = Math.max(0, Math.round((now - ts) / 1000));
  if (s < 10) return 'just now';
  if (s < 60) return `${s}s ago`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h}h ago`;
  return `${Math.round(h / 24)}d ago`;
}

export function clockTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
}

export function dayTime(ts: number): string {
  const d = new Date(ts);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) return clockTime(ts);
  return d.toLocaleDateString([], { weekday: 'short' }) + ' ' + clockTime(ts);
}

/** claude-opus-5-5 → { family: "opus", label: "Opus 5.5" } */
export function modelLabel(id: string | null | undefined): { family: string; label: string } {
  if (!id) return { family: 'unknown', label: '—' };
  const m = /^claude-([a-z]+)-(\d+)(?:-(\d+))?/.exec(id);
  if (!m) return { family: 'unknown', label: id };
  const family = m[1];
  const ver = m[3] ? `${m[2]}.${m[3]}` : m[2];
  return { family, label: `${family[0].toUpperCase()}${family.slice(1)} ${ver}` };
}
