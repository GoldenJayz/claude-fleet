import type { ReactNode } from 'react';
import { isTauri } from '../lib/machines';

async function win() {
  const { getCurrentWindow } = await import('@tauri-apps/api/window');
  return getCurrentWindow();
}

function WinButton({ onClick, label, danger, children }: { onClick: () => void; label: string; danger?: boolean; children: ReactNode }) {
  return (
    <button
      aria-label={label}
      onClick={onClick}
      className={`flex h-10 w-12 items-center justify-center text-white/60 transition-colors hover:text-white ${danger ? 'hover:bg-rose-600/80' : 'hover:bg-white/10'}`}
    >
      {children}
    </button>
  );
}

export function TitleBar({ online, total }: { online: number; total: number }) {
  return (
    <header data-tauri-drag-region className="flex h-10 shrink-0 items-center pl-4">
      <div data-tauri-drag-region className="flex flex-1 items-center gap-2.5">
        <svg width="18" height="18" viewBox="0 0 24 24" className="pointer-events-none">
          <defs>
            <linearGradient id="tb" x1="0" y1="0" x2="1" y2="1">
              <stop offset="0" stopColor="#f0a47a" />
              <stop offset="0.5" stopColor="#c084fc" />
              <stop offset="1" stopColor="#60a5fa" />
            </linearGradient>
          </defs>
          <circle cx="12" cy="12" r="8" fill="none" stroke="url(#tb)" strokeWidth="3" strokeDasharray="36 50" strokeLinecap="round" transform="rotate(-90 12 12)" />
        </svg>
        <span data-tauri-drag-region className="font-display text-[13px] font-semibold tracking-wide text-white/85">
          Claude Fleet
        </span>
        <span data-tauri-drag-region className="text-[12px] text-white/40 tabular">
          {online}/{total} machine{total === 1 ? '' : 's'} online
        </span>
      </div>
      {isTauri && (
        <div className="flex">
          <WinButton label="Minimize" onClick={() => win().then((w) => w.minimize())}>
            <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5h10" stroke="currentColor" /></svg>
          </WinButton>
          <WinButton label="Maximize" onClick={() => win().then((w) => w.toggleMaximize())}>
            <svg width="10" height="10" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" /></svg>
          </WinButton>
          <WinButton label="Close" danger onClick={() => win().then((w) => w.close())}>
            <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" /></svg>
          </WinButton>
        </div>
      )}
    </header>
  );
}
