import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';
import { useCallback, useEffect, useRef, useState } from 'react';
import { shellStatus, shellUrl, type CollectorState } from '../lib/collector';
import { isMobile } from '../lib/machines';
import type { MachineConfig } from '../types';
import { Chip } from './Chip';

type Phase =
  | { kind: 'checking' }
  | { kind: 'disabled'; message: string }
  | { kind: 'connecting' }
  | { kind: 'open'; shell: string }
  | { kind: 'ended'; message: string };

const btn = 'rounded-xl px-3 py-1.5 text-[12.5px] font-medium ring-1 transition-colors disabled:opacity-40';

/** Keys a phone keyboard doesn't have. */
const KEYS: { label: string; seq: string }[] = [
  { label: 'Esc', seq: '\x1b' },
  { label: 'Tab', seq: '\t' },
  { label: 'Ctrl+C', seq: '\x03' },
  { label: '↑', seq: '\x1b[A' },
  { label: '↓', seq: '\x1b[B' },
  { label: '←', seq: '\x1b[D' },
  { label: '→', seq: '\x1b[C' },
];

/**
 * An interactive PowerShell on another PC, through its collector. The session stays open while
 * you look at other pages; "End session" (or closing it from the sidebar) ends the shell there.
 */
export function TerminalView({
  machine,
  state,
  from,
  visible,
  onClose,
}: {
  machine: MachineConfig;
  state?: CollectorState;
  from: string;
  visible: boolean;
  onClose: () => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);
  const fit = useRef<FitAddon | null>(null);
  const ws = useRef<WebSocket | null>(null);
  const [phase, setPhase] = useState<Phase>({ kind: 'checking' });
  const [attempt, setAttempt] = useState(0);
  const name = state?.snapshot?.machine.machineName ?? machine.name;

  // One xterm instance for the life of this view.
  useEffect(() => {
    const t = new Terminal({
      fontFamily: "'Cascadia Mono', 'Cascadia Code', Consolas, monospace",
      fontSize: isMobile ? 12 : 13,
      lineHeight: 1.15,
      cursorBlink: false,
      scrollback: 5000,
      allowProposedApi: false,
      theme: {
        background: '#0b0b12',
        foreground: '#e6e6ef',
        cursor: '#c4b5fd',
        selectionBackground: 'rgba(167,139,250,0.35)',
        black: '#1b1b24',
        brightBlack: '#5c5c70',
        blue: '#7aa2f7',
        brightBlue: '#9ab8ff',
        cyan: '#7dcfff',
        brightCyan: '#a5e1ff',
        green: '#9ece6a',
        brightGreen: '#b9f27c',
        magenta: '#bb9af7',
        brightMagenta: '#d0b8ff',
        red: '#f7768e',
        brightRed: '#ff9aab',
        white: '#c0caf5',
        brightWhite: '#ffffff',
        yellow: '#e0af68',
        brightYellow: '#f5c98a',
      },
    });
    const f = new FitAddon();
    t.loadAddon(f);
    // Ctrl+C copies when text is selected (otherwise it interrupts, as usual); Ctrl+V pastes.
    t.attachCustomKeyEventHandler((e) => {
      if (e.type !== 'keydown' || !e.ctrlKey) return true;
      if (e.key === 'c' && t.hasSelection()) {
        navigator.clipboard.writeText(t.getSelection()).catch(() => {});
        t.clearSelection();
        return false;
      }
      if (e.key === 'v') {
        navigator.clipboard.readText().then((s) => s && t.paste(s)).catch(() => {});
        return false;
      }
      return true;
    });
    t.open(host.current!);
    const enc = new TextEncoder();
    t.onData((d) => ws.current?.readyState === WebSocket.OPEN && ws.current.send(enc.encode(d)));
    t.onBinary((d) => ws.current?.readyState === WebSocket.OPEN && ws.current.send(Uint8Array.from(d, (c) => c.charCodeAt(0))));
    term.current = t;
    fit.current = f;
    return () => {
      t.dispose();
      term.current = null;
    };
  }, []);

  // Keep the remote console the same size as the visible terminal.
  const refit = useCallback(() => {
    const t = term.current;
    if (!t || !host.current || host.current.clientWidth === 0) return;
    fit.current?.fit();
    if (ws.current?.readyState === WebSocket.OPEN) ws.current.send(JSON.stringify({ type: 'resize', cols: t.cols, rows: t.rows }));
  }, []);
  useEffect(() => {
    const ro = new ResizeObserver(() => refit());
    if (host.current) ro.observe(host.current);
    return () => ro.disconnect();
  }, [refit]);
  useEffect(() => {
    if (visible) {
      requestAnimationFrame(() => {
        refit();
        term.current?.focus();
      });
    }
  }, [visible, refit]);

  // Connect (again on Reconnect).
  useEffect(() => {
    let cancelled = false;
    const t = term.current!;
    setPhase({ kind: 'checking' });
    (async () => {
      let shell = 'shell';
      try {
        const st = await shellStatus(machine);
        if (!st.enabled) {
          if (!cancelled) setPhase({ kind: 'disabled', message: `Remote shell is turned off on ${st.machine}. On that PC, open Claude Fleet → Settings → System and turn on “Allow remote shell”.` });
          return;
        }
        shell = st.shell;
      } catch (e) {
        if (!cancelled) setPhase({ kind: 'disabled', message: e instanceof TypeError ? `Can’t reach ${name}.` : String((e as Error).message ?? e) });
        return;
      }
      if (cancelled) return;
      setPhase({ kind: 'connecting' });
      refit();
      const sock = new WebSocket(shellUrl(machine, t.cols || 120, t.rows || 30, from));
      sock.binaryType = 'arraybuffer';
      ws.current = sock;
      let opened = false;
      let ended: string | null = null;
      sock.onopen = () => {
        opened = true;
        if (attempt > 0) t.write('\r\n');
        setPhase({ kind: 'open', shell });
        refit();
        t.focus();
      };
      sock.onmessage = (m) => {
        if (typeof m.data === 'string') {
          try {
            const msg = JSON.parse(m.data) as { type: string; code?: number; message?: string };
            if (msg.type === 'exit') ended = `The shell exited${msg.code ? ` (code ${msg.code})` : ''}.`;
            if (msg.type === 'error') ended = msg.message ?? 'The shell could not start.';
          } catch {
            /* ignore */
          }
          return;
        }
        t.write(new Uint8Array(m.data as ArrayBuffer));
      };
      sock.onclose = () => {
        if (ws.current === sock) ws.current = null;
        if (cancelled) return;
        setPhase({ kind: 'ended', message: ended ?? (opened ? `Disconnected from ${name}.` : `Couldn’t open a shell on ${name}.`) });
      };
    })();
    return () => {
      cancelled = true;
      ws.current?.close();
      ws.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [machine.address, machine.token, attempt]);

  const open = phase.kind === 'open';
  const send = (s: string) => {
    if (ws.current?.readyState === WebSocket.OPEN) ws.current.send(new TextEncoder().encode(s));
  };

  return (
    <section className={`glass flex flex-col rounded-[28px] ${isMobile ? 'h-[calc(100dvh-150px)] min-h-[240px] p-2.5' : 'h-[calc(100vh-170px)] min-h-[420px] p-4'} ${visible ? '' : 'hidden'}`}>
      <header className="mb-3 flex items-center justify-between gap-3 px-1">
        <div className="flex min-w-0 items-center gap-3">
          <h2 className="truncate font-display text-xl font-semibold">{isMobile ? name : `Terminal · ${name}`}</h2>
          {open ? (
            <Chip tone="live">{phase.shell}</Chip>
          ) : phase.kind === 'connecting' || phase.kind === 'checking' ? (
            <Chip tone="muted">connecting…</Chip>
          ) : (
            <Chip tone="bad">not connected</Chip>
          )}
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {!open && phase.kind !== 'checking' && phase.kind !== 'connecting' && (
            <button className={`${btn} bg-violet-500/30 text-violet-50 ring-violet-300/40 hover:bg-violet-500/45`} onClick={() => setAttempt((a) => a + 1)}>
              Reconnect
            </button>
          )}
          <button className={`${btn} bg-white/[0.06] text-white/80 ring-white/10 hover:bg-rose-500/30`} onClick={onClose}>
            {open ? 'End session' : 'Close'}
          </button>
        </div>
      </header>
      {(phase.kind === 'disabled' || phase.kind === 'ended') && (
        <div className={`mb-3 rounded-xl px-3 py-2 text-[12.5px] ${phase.kind === 'disabled' ? 'bg-amber-500/10 text-amber-100' : 'bg-white/[0.05] text-white/70'}`}>{phase.message}</div>
      )}
      <div className="min-h-0 flex-1 overflow-hidden rounded-2xl bg-[#0b0b12] p-2 ring-1 ring-white/10" style={{ userSelect: 'text' }}>
        <div ref={host} className="h-full w-full" />
      </div>
      {isMobile && open && (
        <div className="thin-scroll mt-2 flex shrink-0 gap-1.5 overflow-x-auto">
          {KEYS.map((k) => (
            <button
              key={k.label}
              // Keep the keyboard up: don't let the button take focus from the terminal.
              onPointerDown={(e) => e.preventDefault()}
              onClick={() => {
                send(k.seq);
                term.current?.focus();
              }}
              className="shrink-0 rounded-lg bg-white/[0.08] px-3 py-2 font-mono text-[12.5px] text-white/85 ring-1 ring-white/10 active:bg-white/20"
            >
              {k.label}
            </button>
          ))}
          <button
            onPointerDown={(e) => e.preventDefault()}
            onClick={() => navigator.clipboard.readText().then((s) => s && term.current?.paste(s)).catch(() => {})}
            className="shrink-0 rounded-lg bg-white/[0.08] px-3 py-2 text-[12.5px] text-white/85 ring-1 ring-white/10 active:bg-white/20"
          >
            Paste
          </button>
        </div>
      )}
      <div className={`mt-2 px-1 text-[11px] text-white/35 ${isMobile ? 'hidden' : ''}`}>
        Runs as the signed-in user on {name}, without admin rights. Ctrl+C copies selected text, Ctrl+V pastes. Ending the session closes the shell and anything still running in it.
      </div>
    </section>
  );
}
