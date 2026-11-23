import { useEffect } from 'react';
import type { PlanView } from './plan';
import { system } from './system';

const KEY = 'claude-fleet.alerted.v1';
export const ALERT_AT = 80;

/**
 * One Windows notification per limit window when real usage passes 80%.
 * Keyed by window + reset time, so it fires again after the window resets.
 */
export function usePlanAlert(plan: PlanView, enabled: boolean) {
  useEffect(() => {
    if (!enabled || !plan.reading) return;
    let seen: string[] = [];
    try {
      seen = JSON.parse(localStorage.getItem(KEY) ?? '[]');
    } catch {
      /* start fresh */
    }
    for (const w of plan.windows) {
      if (w.used == null || w.used < ALERT_AT || !w.resetsAt) continue;
      const id = `${w.key}:${w.resetsAt}`;
      if (seen.includes(id)) continue;
      seen = [...seen.slice(-20), id];
      localStorage.setItem(KEY, JSON.stringify(seen));
      const left = Math.max(0, 100 - Math.round(w.used));
      const reset = new Date(w.resetsAt).toLocaleString([], { weekday: 'short', hour: 'numeric', minute: '2-digit' });
      system
        .notify(
          `Claude ${w.label.toLowerCase()} limit at ${Math.round(w.used)}%`,
          `About ${left}% left. Resets ${reset}.${w.atLeast ? ' (From an older reading; it may be higher now.)' : ''}`,
        )
        .catch(() => {});
    }
  }, [plan, enabled]);
}
