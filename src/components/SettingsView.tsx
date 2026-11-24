import { useEffect, useState, type ReactNode } from 'react';
import type { CollectorState } from '../lib/collector';
import { isDesktopApp, isMobile, loadAddresses, pairingCode, type LanAddress, type LocalCollector } from '../lib/machines';
import type { MachineConfig } from '../types';
import { MachinesSettings } from './MachinesSettings';
import { SystemSettings } from './SystemSettings';
import type { UiSettings, WindowEffect } from '../lib/settings';
import { Chip } from './Chip';
import { isTauri } from '../lib/machines';
import type { Updates } from '../lib/updates';

function Section({ title, desc, children }: { title: string; desc?: string; children: ReactNode }) {
  return (
    <section className="glass rounded-3xl p-5">
      <h2 className="font-display text-[15px] font-semibold">{title}</h2>
      {desc && <p className="mt-0.5 text-[12.5px] text-white/50">{desc}</p>}
      <div className="mt-4 flex flex-col gap-4">{children}</div>
    </section>
  );
}

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
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

function Segmented<T extends string>({ value, options, onChange }: { value: T; options: { value: T; label: string }[]; onChange: (v: T) => void }) {
  return (
    <div className="flex rounded-xl bg-white/[0.06] p-0.5 ring-1 ring-white/10">
      {options.map((o) => (
        <button
          key={o.value}
          onClick={() => onChange(o.value)}
          className={`rounded-[10px] px-3 py-1 text-[12px] transition-colors ${value === o.value ? 'bg-white/15 text-white' : 'text-white/55 hover:text-white'}`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

function Toggle({ on, onChange }: { on: boolean; onChange: (v: boolean) => void }) {
  return (
    <button
      role="switch"
      aria-checked={on}
      onClick={() => onChange(!on)}
      className={`relative h-6 w-11 rounded-full transition-colors ${on ? 'bg-violet-500/70' : 'bg-white/15'}`}
    >
      <span className={`absolute top-0.5 h-5 w-5 rounded-full bg-white shadow transition-all ${on ? 'left-[22px]' : 'left-0.5'}`} />
    </button>
  );
}

function CopyButton({ text, label = 'Copy' }: { text: string; label?: string }) {
  const [done, setDone] = useState(false);
  return (
    <button
      onClick={() => navigator.clipboard.writeText(text).then(() => {
        setDone(true);
        setTimeout(() => setDone(false), 1500);
      })}
      className="rounded-lg bg-white/[0.08] px-2.5 py-1 text-[12px] text-white/80 ring-1 ring-white/10 hover:bg-white/[0.14]"
    >
      {done ? 'Copied' : label}
    </button>
  );
}

function AboutSection({ updates }: { updates: Updates }) {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    if (isTauri) import('@tauri-apps/api/app').then((m) => m.getVersion()).then(setVersion).catch(() => {});
  }, []);
  const p = updates.phase;
  const status =
    p.kind === 'checking'
      ? 'Checking…'
      : p.kind === 'upToDate'
        ? 'You have the latest version.'
        : p.kind === 'available'
          ? `Version ${p.info.version} is available.`
          : p.kind === 'downloading' || p.kind === 'installing'
            ? 'Updating…'
            : p.kind === 'error' && !p.info
              ? `Couldn’t check: ${p.message}`
              : 'Checks automatically every few hours.';
  return (
    <Section title="About">
      <Row label={`Claude Fleet ${version ?? ''}`} hint={status}>
        {p.kind === 'available' ? (
          <button onClick={updates.install} className="rounded-lg bg-violet-500/35 px-2.5 py-1 text-[12px] text-violet-50 ring-1 ring-violet-300/40 hover:bg-violet-500/50">
            Update now
          </button>
        ) : (
          <button
            disabled={!isTauri || p.kind === 'checking' || p.kind === 'downloading' || p.kind === 'installing'}
            onClick={() => updates.check(true)}
            className="rounded-lg bg-white/[0.08] px-2.5 py-1 text-[12px] text-white/80 ring-1 ring-white/10 hover:bg-white/[0.14] disabled:opacity-40"
          >
            Check for updates
          </button>
        )}
      </Row>
    </Section>
  );
}

export function SettingsView({
  updates,
  settings,
  update,
  local,
  localState,
  saved,
  states,
  onMachinesChange,
}: {
  updates: Updates;
  settings: UiSettings;
  update: (p: Partial<UiSettings>) => void;
  local: LocalCollector | null;
  localState?: CollectorState;
  saved: MachineConfig[];
  states: Record<string, CollectorState>;
  onMachinesChange: (list: MachineConfig[]) => Promise<void>;
}) {
  const [reveal, setReveal] = useState(false);
  const [addresses, setAddresses] = useState<LanAddress[]>([]);
  const [addrIdx, setAddrIdx] = useState(0);
  useEffect(() => {
    loadAddresses().then(setAddresses).catch(() => {});
  }, []);
  const chosen = addresses[addrIdx];
  const snap = localState?.snapshot;
  const token = local?.token ?? '';

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-4">
      {isMobile && (
        <Section title="Machines" desc="Your PCs running Claude Fleet. On each PC, open Settings → This PC → Copy code, get the code to this phone, and paste it here.">
          <MachinesSettings saved={saved} states={states} onChange={onMachinesChange} />
        </Section>
      )}

      <Section title="Appearance" desc={isMobile ? undefined : 'How the window sits on your desktop.'}>
        {!isMobile && (<>
        <Row label="Transparency" hint="Lower is more see-through. Cards stay a little darker so text is readable.">
          <span className="w-10 text-right text-[12px] text-white/55 tabular">{Math.round((1 - settings.tint) * 100)}%</span>
          <input
            type="range"
            className="slider w-52"
            min={0}
            max={90}
            value={Math.round((1 - settings.tint) * 100)}
            onChange={(e) => update({ tint: 1 - Number(e.target.value) / 100 })}
          />
        </Row>
        <Row label="Window backdrop" hint={isDesktopApp ? 'Blur: Acrylic. Wallpaper tint: Mica. Clear shows your desktop sharp.' : 'Only available in the desktop app.'}>
          <Segmented<WindowEffect>
            value={settings.effect}
            onChange={(effect) => update({ effect })}
            options={[
              { value: 'acrylic', label: 'Blur' },
              { value: 'mica', label: 'Mica' },
              { value: 'clear', label: 'Clear' },
            ]}
          />
        </Row>
        </>)}
        <Row label="Reduce motion" hint="Turns off animations and pulsing indicators.">
          <Toggle on={settings.reduceMotion} onChange={(reduceMotion) => update({ reduceMotion })} />
        </Row>
      </Section>

      {!isMobile && (<>
      <Section title="This PC" desc="Other machines use these details to show this PC on their dashboard.">
        <Row label="Machine name">
          <span className="text-[13px] text-white/80">{local?.machineName ?? '–'}</span>
        </Row>
        <Row label="Collector" hint={local?.embedded ? 'Running inside this app.' : 'Using a separately running collector.'}>
          {local?.error ? <Chip tone="bad">{local.error}</Chip> : localState?.status === 'online' ? <Chip tone="live">running on port {local?.port}</Chip> : <Chip tone="muted">{localState?.status ?? 'unknown'}</Chip>}
        </Row>
        <Row label="Pairing token" hint="Anyone with this token on your network can read this PC’s stats and prompts. Keep it private.">
          <code className="max-w-[260px] truncate rounded-lg bg-black/30 px-2 py-1 font-mono text-[11.5px] text-white/75 ring-1 ring-white/10">
            {reveal ? token : '•'.repeat(24)}
          </code>
          <button onClick={() => setReveal((r) => !r)} className="rounded-lg px-2 py-1 text-[12px] text-white/60 hover:text-white">
            {reveal ? 'Hide' : 'Show'}
          </button>
          {token && <CopyButton text={token} />}
        </Row>
        <Row
          label="Pairing code"
          hint={
            addresses.length
              ? 'Paste this into Settings → Machines → Add machine on the other PC. It contains the token.'
              : 'Shown in the desktop app.'
          }
        >
          {addresses.length > 0 && local && (
            <>
              <select
                value={addrIdx}
                onChange={(e) => setAddrIdx(Number(e.target.value))}
                className="max-w-[230px] rounded-lg bg-black/30 px-2 py-1 text-[12px] text-white/80 ring-1 ring-white/10 outline-none"
                title="Pick the network the other PC is on"
              >
                {addresses.map((a, i) => (
                  <option key={a.ip} value={i} className="bg-neutral-900">
                    {a.ip} · {a.kind === 'tailscale' ? 'Tailscale' : a.kind === 'virtual' ? `virtual (${a.interface})` : a.interface}
                  </option>
                ))}
              </select>
              {chosen && <CopyButton text={pairingCode(`${chosen.ip}:${local.port}`, local.token, local.machineName)} label="Copy code" />}
            </>
          )}
        </Row>
        <Row label="Plan usage source" hint="Claude Code’s status line saves your 5-hour and weekly usage for the banner.">
          {snap ? (snap.statusLine.installed ? <Chip tone="live">status line installed</Chip> : <Chip tone="warn">not installed</Chip>) : <Chip tone="muted">–</Chip>}
        </Row>
        {local?.dataDir && (
          <Row label="Data folder">
            <span className="max-w-[340px] truncate text-[12px] text-white/55" title={local.dataDir}>
              {local.dataDir}
            </span>
          </Row>
        )}
      </Section>

      <Section title="System" desc="Windows options for this PC. Nothing here changes until you click it.">
        <SystemSettings local={local} statusLineInstalled={snap ? snap.statusLine.installed : null} settings={settings} update={update} />
      </Section>

      <Section title="Machines" desc="Other PCs running Claude Fleet. Each gets its own card and sidebar entry.">
        <MachinesSettings saved={saved} states={states} onChange={onMachinesChange} />
      </Section>
      </>)}

      <AboutSection updates={updates} />

      {isMobile && (
        <p className="px-2 text-[11.5px] leading-relaxed text-white/40">
          This phone only views your PCs; it runs no collector. It talks to them directly over your Wi-Fi (or Tailscale when you’re away), and nothing goes through a server. Pairing codes are kept in this app’s private storage.
        </p>
      )}
    </div>
  );
}
