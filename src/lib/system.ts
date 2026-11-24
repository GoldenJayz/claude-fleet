import { isDesktopApp, isTauri } from './machines';

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const core = await import('@tauri-apps/api/core');
  return core.invoke<T>(cmd, args);
}

export interface FirewallStatus {
  exists: boolean;
  enabled: boolean;
  port: string | null;
  program: string | null;
  matchesThisApp: boolean;
}

export interface StatusLineChange {
  installed: boolean;
  backup: string | null;
  chained: string | null;
  settingsPath: string;
}

export const system = {
  available: isDesktopApp,
  firewallStatus: () => invoke<FirewallStatus>('firewall_status'),
  firewallAllow: (port: number) => invoke<FirewallStatus>('firewall_allow', { port }),
  firewallRemove: () => invoke<FirewallStatus>('firewall_remove'),
  statusLineInstall: () => invoke<StatusLineChange>('statusline_install'),
  statusLineUninstall: () => invoke<StatusLineChange>('statusline_uninstall'),
  getAutostart: () => invoke<boolean>('get_autostart'),
  setAutostart: (enabled: boolean) => invoke<boolean>('set_autostart', { enabled }),
  getRemoteShell: () => invoke<boolean>('get_remote_shell'),
  setRemoteShell: (enabled: boolean) => invoke<boolean>('set_remote_shell', { enabled }),
  inboxDir: () => invoke<string>('inbox_dir'),
  openInbox: () => invoke<void>('open_inbox'),
  revealReceived: (path: string) => invoke<void>('reveal_received', { path }),
  notify: async (title: string, body: string) => {
    if (isTauri) return invoke<void>('notify', { title, body });
    if ('Notification' in window && Notification.permission === 'granted') new Notification(title, { body });
  },
};
