import { useEffect, useState, type ReactNode } from 'react';
import type { LocalCollector } from '../lib/machines';
import type { UiSettings } from '../lib/settings';
import { system, type FirewallStatus } from '../lib/system';
import { Chip } from './Chip';

const btn = 'rounded-xl px-3 py-1.5 text-[12.5px] font-medium ring-1 transition-colors disabled:opacity-40';
const primary = `${btn} bg-violet-500/30 text-violet-50 ring-violet-300/40 hover:bg-violet-500/45`;
const quiet = `${btn} bg-white/[0.06] text-white/80 ring-white/10 hover:bg-white/[0.12]`;

function Row({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-6">
      <div className="min-w-0">
        <div className="text-[13px] text-white/90">{label}</div>
        {hint && <div className="text-[11.5px] text-white/45">{hint}</div>}
      </div>
      <div className="flex shrink-0 items-center gap-2">{children}</div>
    </div>
  );
}

function Toggle({ on, onChange, disabled }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      disabled={disabled}
      onClick={() => onChange(!on)}
      className={`relative h-6 w-11 rounded-full transition-colors disabled:opacity-40 ${on ? 'bg-violet-500/70' : 'bg-white/15'}`}
    >
      <span className={`absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition-all ${on ? 'left-[22px]' : 'left-0.5'}`} />
    </button>
  );
}

/** Windows-level options. Each change happens only when you click it here. */
export function SystemSettings({
  local,
  statusLineInstalled,
  settings,
  update,
}: {
  local: LocalCollector | null;
  statusLineInstalled: boolean | null;
  settings: UiSettings;
  update: (p: Partial<UiSettings>) => void;
}) {
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [fw, setFw] = useState<FirewallStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [sl, setSl] = useState<boolean | null>(statusLineInstalled);

  useEffect(() => setSl(statusLineInstalled), [statusLineInstalled]);
  useEffect(() => {
    if (!system.available) return;
    system.getAutostart().then(setAutostart).catch(() => {});
    system.firewallStatus().then(setFw).catch(() => {});
  }, []);

  async function act<T>(key: string, f: () => Promise<T>, done: (r: T) => string) {
    setBusy(key);
    setMsg(null);
    try {
      const r = await f();
      setMsg({ ok: true, text: done(r) });
    } catch (e) {
      setMsg({ ok: false, text: String(e) });
    } finally {
      setBusy(null);
    }
  }

  const port = local?.port ?? 47821;
  const fwOk = fw?.exists && fw.enabled && fw.matchesThisApp;

  return (
    <div className="flex flex-col gap-4">
      {!system.available && <div className="text-[12.5px] text-white/50">These options are available in the desktop app.</div>}
      <Row label="Start with Windows" hint="Opens in the tray when you sign in, so other PCs can always see this one.">
        <Toggle
          on={!!autostart}
          disabled={!system.available || autostart === null || busy === 'autostart'}
          onChange={(v) => act('autostart', () => system.setAutostart(v).then((r) => (setAutostart(r), r)), (r) => (r ? 'Claude Fleet will start with Windows.' : 'Claude Fleet won’t start with Windows.'))}
        />
      </Row>

      <Row label="Alert at 80% plan usage" hint="A Windows notification when the 5-hour or weekly limit passes 80% (once per window).">
        <Toggle on={settings.planAlert} onChange={(planAlert) => update({ planAlert })} />
      </Row>

      <Row
        label="Allow other PCs to connect"
        hint={
          <>
            Adds a Windows Firewall rule for port {port}: this app only, local network and Tailscale only, Private/Domain networks.
            Windows asks for admin approval.
          </>
        }
      >
        {fw === null ? (
          <Chip tone="muted">{system.available ? 'checking…' : '–'}</Chip>
        ) : fwOk ? (
          <>
            <Chip tone="live">allowed</Chip>
            <button
              className={quiet}
              disabled={!!busy}
              onClick={() => act('fw', () => system.firewallRemove().then((r) => (setFw(r), r)), () => 'Firewall rule removed.')}
            >
              Remove
            </button>
          </>
        ) : (
          <>
            {fw.exists && <Chip tone="warn">{fw.matchesThisApp ? 'rule disabled' : 'rule is for another copy'}</Chip>}
            <button
              className={primary}
              disabled={!system.available || !!busy}
              onClick={() =>
                act(
                  'fw',
                  () => system.firewallAllow(port).then((r) => (setFw(r), r)),
                  (r) => (r.exists ? `Allowed: other PCs on your network can reach port ${port}.` : 'No rule was added.'),
                )
              }
            >
              {busy === 'fw' ? 'Waiting for Windows…' : fw.exists ? 'Fix rule' : 'Allow'}
            </button>
          </>
        )}
      </Row>

      <Row
        label="Claude Code status line"
        hint="Feeds plan usage to the banner. Merges into ~/.claude/settings.json with a backup; an existing status line keeps working."
      >
        {sl ? (
          <>
            <Chip tone="live">installed</Chip>
            <button
              className={quiet}
              disabled={!system.available || !!busy}
              onClick={() => act('sl', () => system.statusLineUninstall().then((r) => (setSl(false), r)), (r) => `Removed. Backup: ${r.backup ?? 'none needed'}`)}
            >
              Remove
            </button>
          </>
        ) : (
          <button
            className={primary}
            disabled={!system.available || !!busy || sl === null}
            onClick={() =>
              act(
                'sl',
                () => system.statusLineInstall().then((r) => (setSl(true), r)),
                (r) => `Installed in ${r.settingsPath}${r.backup ? `. Backup: ${r.backup}` : ''}${r.chained ? `. Your previous status line still runs.` : ''}`,
              )
            }
          >
            Install
          </button>
        )}
      </Row>

      {msg && (
        <div className={`rounded-xl px-3 py-2 text-[12.5px] ${msg.ok ? 'bg-emerald-500/10 text-emerald-100' : 'bg-rose-500/10 text-rose-100'}`}>{msg.text}</div>
      )}
    </div>
  );
}
