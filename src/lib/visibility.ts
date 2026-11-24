import { useEffect, useState } from 'react';
import { isDesktopApp } from './machines';

/**
 * Whether the dashboard is on screen. While it's hidden in the tray (or minimized) the UI stops
 * redrawing: live data is still received and kept, but React only re-renders once it's back.
 *
 * In the Windows app this asks the window itself (shown and not minimized). The page's own
 * `document.visibilityState` isn't trusted there: the window starts hidden and is shown during
 * startup, and the page can keep reporting "hidden" afterwards, which froze the dashboard.
 * When in doubt we report visible, since a stuck "hidden" is far worse than a little extra work.
 */
let visible = true;
const subs = new Set<(v: boolean) => void>();

function set(v: boolean) {
  if (v === visible) return;
  visible = v;
  subs.forEach((f) => f(v));
}

if (isDesktopApp) {
  import('@tauri-apps/api/window').then(async ({ getCurrentWindow }) => {
    const w = getCurrentWindow();
    const check = async () => {
      try {
        const [shown, minimized] = await Promise.all([w.isVisible(), w.isMinimized()]);
        set(shown && !minimized);
      } catch {
        set(true);
      }
    };
    await check();
    // Hide/show from the tray, minimize/restore, and focus changes all re-check.
    const { listen } = await import('@tauri-apps/api/event');
    listen<boolean>('fleet://visible', () => check());
    w.onResized(() => check());
    w.onFocusChanged(() => check());
    // Belt and braces: anything else that changes visibility is caught within a few seconds.
    setInterval(check, 3000);
  });
} else if (typeof document !== 'undefined') {
  visible = document.visibilityState === 'visible';
  document.addEventListener('visibilitychange', () => set(document.visibilityState === 'visible'));
}

export const isVisible = () => visible;

export function onVisibility(f: (v: boolean) => void) {
  subs.add(f);
  return () => {
    subs.delete(f);
  };
}

export function useVisible() {
  const [v, setV] = useState(visible);
  useEffect(() => onVisibility(setV), []);
  return v;
}
