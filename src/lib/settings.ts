import { useCallback, useEffect, useState } from 'react';
import { isDesktopApp } from './machines';

export type WindowEffect = 'acrylic' | 'mica' | 'clear';

export interface UiSettings {
  /** 0 = fully see-through, 1 = solid. Tint over the desktop behind the window. */
  tint: number;
  effect: WindowEffect;
  sidebarCollapsed: boolean;
  reduceMotion: boolean;
  /** Windows notification when plan usage passes 80%. */
  planAlert: boolean;
}

const KEY = 'claude-fleet.ui.v1';
const DEFAULTS: UiSettings = { tint: 0.4, effect: 'acrylic', sidebarCollapsed: false, reduceMotion: false, planAlert: true };

function load(): UiSettings {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? '{}') };
  } catch {
    return DEFAULTS;
  }
}

/** Push the tint into CSS. Cards get a bit more opacity than the window so text stays readable. */
function applyTint(s: UiSettings) {
  const root = document.documentElement.style;
  const t = isDesktopApp ? s.tint : Math.max(s.tint, 0.94);
  root.setProperty('--tint', t.toFixed(3));
  root.setProperty('--card', Math.min(0.92, 0.3 + t * 0.55).toFixed(3));
  root.setProperty('--tile', Math.min(0.6, 0.12 + t * 0.25).toFixed(3));
}

async function applyEffect(effect: WindowEffect) {
  if (!isDesktopApp) return;
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('set_window_effect', { effect }).catch(() => {});
}

export function useUiSettings() {
  const [settings, setSettings] = useState<UiSettings>(load);

  useEffect(() => {
    applyTint(settings);
    document.documentElement.classList.toggle('reduce-motion', settings.reduceMotion);
    localStorage.setItem(KEY, JSON.stringify(settings));
  }, [settings]);

  useEffect(() => {
    applyEffect(settings.effect);
  }, [settings.effect]);

  const update = useCallback((patch: Partial<UiSettings>) => setSettings((s) => ({ ...s, ...patch })), []);
  return [settings, update] as const;
}
