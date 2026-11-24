import { bytes } from '../lib/format';
import { isMobile } from '../lib/machines';
import type { Updates } from '../lib/updates';

const btn = 'shrink-0 rounded-lg px-3 py-1.5 text-[12px] font-medium ring-1 transition-colors';

/** "Claude Fleet x.y.z is available" with Install, progress, and what happens next. */
export function UpdateBanner({ updates }: { updates: Updates }) {
  const { phase, install, dismiss, showBanner } = updates;
  if (!showBanner || !('info' in phase) || !phase.info) return null;
  const { info } = phase;

  let text: string;
  let action: { label: string; run: () => void } | null = null;
  switch (phase.kind) {
    case 'available':
      text = `Claude Fleet ${info.version} is available (you have ${info.current}).`;
      action = { label: isMobile ? 'Update' : 'Install & restart', run: install };
      break;
    case 'downloading':
      text = `Downloading ${info.version}… ${bytes(phase.got)}${phase.total ? ` of ${bytes(phase.total)}` : ''}`;
      break;
    case 'installing':
      text = isMobile ? 'Confirm the update in the Android prompt.' : 'Installing… Claude Fleet will restart (Windows may ask for permission).';
      break;
    case 'needsPermission':
      text = 'Allow “Install unknown apps” for Claude Fleet in the screen that opened, then come back and tap Install.';
      action = { label: 'Install', run: install };
      break;
    case 'error':
      text = `Update failed: ${phase.message}`;
      action = { label: 'Retry', run: install };
      break;
    default:
      return null;
  }
  const pct = phase.kind === 'downloading' && phase.total ? Math.min(100, (phase.got * 100) / phase.total) : null;

  return (
    <div className="glass shrink-0 overflow-hidden rounded-2xl">
      <div className="flex items-center gap-3 px-4 py-2.5 text-[12.5px]">
        <span className="h-2 w-2 shrink-0 rounded-full bg-violet-300 shadow-[0_0_8px_rgba(196,181,253,0.8)]" />
        <span className={`min-w-0 flex-1 ${phase.kind === 'error' ? 'text-rose-100' : 'text-white/85'}`}>{text}</span>
        {action && (
          <button className={`${btn} bg-violet-500/35 text-violet-50 ring-violet-300/40 hover:bg-violet-500/50`} onClick={action.run}>
            {action.label}
          </button>
        )}
        {phase.kind === 'available' && (
          <button className={`${btn} text-white/60 ring-white/10 hover:text-white`} onClick={dismiss}>
            Later
          </button>
        )}
      </div>
      {phase.kind === 'downloading' && (
        <div className="h-1 bg-white/[0.06]">
          <div className="h-full bg-gradient-to-r from-violet-300 to-sky-400 transition-[width] duration-300" style={{ width: `${pct ?? 15}%` }} />
        </div>
      )}
    </div>
  );
}
