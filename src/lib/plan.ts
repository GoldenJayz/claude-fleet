import { useMemo } from 'react';
import type { PlanReading } from '../types';

/** A reading newer than this is shown as current (Anthropic is asked every 5 minutes). */
export const FRESH_MS = 10 * 60_000;

/*
 * Why there is no estimate: plan usage is shared by everything on the account (claude.ai, the
 * phone app, every machine). The logs on these PCs only see Claude Code, so extrapolating from
 * them was badly wrong (tested against two real readings: predicted 250–340% vs. a real 92%).
 * We show only what is known: the last real reading, which is a lower bound until its window resets.
 */

export interface WindowView {
  key: 'fiveHour' | 'sevenDay';
  label: string;
  /** 0–100; null when unknown (the window reset after the last reading). */
  used: number | null;
  resetsAt: number | null;
  /** The reading is old but its window hasn't reset: real usage is at least `used`. */
  atLeast: boolean;
  /** The window reset after the last reading. */
  resetSinceReading: boolean;
}

export interface PlanView {
  reading: PlanReading | null;
  /** Machine that produced the reading. */
  source: string | null;
  fresh: boolean;
  windows: WindowView[];
}

/** Plan usage is per account: take the newest reading any machine has. */
export function latestReading(readings: { machine: string; plan: PlanReading | null }[]) {
  let best: { machine: string; plan: PlanReading } | null = null;
  for (const r of readings) {
    if (r.plan && (!best || r.plan.capturedAtMs > best.plan.capturedAtMs)) best = { machine: r.machine, plan: r.plan };
  }
  return best;
}

export function usePlan(readings: { machine: string; plan: PlanReading | null }[], now: number): PlanView {
  const best = useMemo(() => latestReading(readings), [readings]);
  const fresh = !!best && now - best.plan.capturedAtMs < FRESH_MS;

  const windows: WindowView[] = (
    [
      ['fiveHour', '5-hour session'],
      ['sevenDay', 'Weekly'],
    ] as const
  ).map(([key, label]) => {
    const w = best?.plan[key] ?? null;
    if (!w) return { key, label, used: null, resetsAt: null, atLeast: false, resetSinceReading: false };
    if (now >= w.resetsAtMs) {
      return { key, label, used: null, resetsAt: w.resetsAtMs, atLeast: false, resetSinceReading: true };
    }
    return { key, label, used: w.usedPercentage, resetsAt: w.resetsAtMs, atLeast: !fresh, resetSinceReading: false };
  });

  return { reading: best?.plan ?? null, source: best?.machine ?? null, fresh, windows };
}
