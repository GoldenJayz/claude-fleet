import { AnimatePresence, motion } from "framer-motion";
import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Background } from "./components/Background";
import { DropOverlay } from "./components/DropOverlay";
import { AgentsView } from "./components/AgentsView";
import { MachineCard } from "./components/MachineCard";
import { MobileHeader, MobileNav } from "./components/MobileNav";
import { ScreenView } from "./components/ScreenView";
import { Overview } from "./components/Overview";
import { PlanBanner } from "./components/PlanBanner";
import { SettingsView } from "./components/SettingsView";
import { Sidebar, type View } from "./components/Sidebar";
import { TitleBar } from "./components/TitleBar";
import { TransfersTray } from "./components/TransfersTray";
import { UpdateBanner } from "./components/UpdateBanner";
import {
  useCollector,
  useNow,
  type CollectorEvent,
  type CollectorState,
} from "./lib/collector";
import { bytes } from "./lib/format";
import { isMobile, useMachines } from "./lib/machines";
import { usePlan } from "./lib/plan";
import { publishTaskStep } from "./lib/agent";
import { usePlanAlert } from "./lib/alerts";
import { useUiSettings } from "./lib/settings";
import { system } from "./lib/system";
import { useTransfers } from "./lib/transfers";
import { useUpdates } from "./lib/updates";
import type { MachineConfig, ShellSession } from "./types";

// xterm.js is only loaded once a terminal is opened.
const TerminalView = lazy(() =>
  import("./components/TerminalView").then((m) => ({
    default: m.TerminalView,
  })),
);

/** Keeps one live connection per machine regardless of which view is showing. */
function Connection({
  machine,
  onState,
  onEvent,
}: {
  machine: MachineConfig;
  onState: (id: string, s: CollectorState) => void;
  onEvent?: (ev: CollectorEvent) => void;
}) {
  const state = useCollector(machine, onEvent);
  useEffect(() => onState(machine.id, state), [machine.id, state, onState]);
  return null;
}

export default function App() {
  const { machines, loaded, local, saved, updateSaved } = useMachines();
  const [settings, update] = useUiSettings();
  const now = useNow(1000);
  const [states, setStates] = useState<Record<string, CollectorState>>({});
  const [view, setViewRaw] = useState<View>({ kind: "overview" });
  // On the phone, Android's Back button returns to the overview instead of closing the app:
  // leaving the overview adds one history entry, and Back (popstate) comes home.
  const awayFromHome = useRef(false);
  useEffect(() => {
    if (!isMobile) return;
    const onPop = () => {
      awayFromHome.current = false;
      setViewRaw({ kind: "overview" });
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);
  const setView = useCallback((v: View | ((prev: View) => View)) => {
    setViewRaw((prev) => {
      const next = typeof v === "function" ? v(prev) : v;
      if (isMobile) {
        if (next.kind !== "overview" && !awayFromHome.current) {
          awayFromHome.current = true;
          history.pushState({ fleet: 1 }, "");
        } else if (next.kind === "overview" && awayFromHome.current) {
          awayFromHome.current = false;
          history.back();
        }
      }
      return next;
    });
  }, []);
  const [terminals, setTerminals] = useState<string[]>([]);
  const [screenOf, setScreenOf] = useState<string | null>(null);
  const screenNotified = useRef<Record<string, number>>({});
  /** Remote shells other PCs have open on this one. */
  const [incomingShells, setIncomingShells] = useState<ShellSession[]>([]);
  const me = local?.machineName ?? (isMobile ? "Phone" : "dashboard");
  const transfers = useTransfers(me);
  const updates = useUpdates();

  const onState = useCallback((id: string, s: CollectorState) => {
    setStates((prev) => (prev[id] === s ? prev : { ...prev, [id]: s }));
  }, []);

  // Events from this PC's own collector: files received, remote shells opened/closed.
  const { received } = transfers;
  const onLocalEvent = useCallback(
    (ev: CollectorEvent) => {
      if (ev.type === "fileReceived") {
        received(ev);
        system
          .notify(
            `Received ${ev.name}`,
            `From ${ev.from} · ${bytes(ev.size)} · saved in Downloads\\Claude Fleet`,
          )
          .catch(() => {});
      } else if (ev.type === "screenViewed") {
        // One notification per viewer every 10 minutes (auto-refresh takes many screenshots).
        const last = screenNotified.current[ev.from] ?? 0;
        if (Date.now() - last > 10 * 60_000) {
          screenNotified.current[ev.from] = Date.now();
          system
            .notify("Screenshot taken", `${ev.from} (${ev.ip}) is viewing this PC's screen.`)
            .catch(() => {});
        }
      } else if (ev.type === "shell") {
        setIncomingShells((list) =>
          ev.ended
            ? list.filter((s) => s.id !== ev.id)
            : [...list.filter((s) => s.id !== ev.id), ev],
        );
        if (!ev.ended)
          system
            .notify(
              "Remote shell opened",
              `${ev.from} (${ev.ip}) opened a PowerShell on this PC.`,
            )
            .catch(() => {});
      }
    },
    [received],
  );

  // Agent task progress from every machine; your own finished tasks get a notification.
  const onAnyEvent = useCallback(
    (m: MachineConfig, ev: CollectorEvent) => {
      if (ev.type === "taskStep") return publishTaskStep(m.id, ev.taskId, ev.step);
      if (ev.type === "task") {
        if (ev.from === me && ["done", "failed"].includes(ev.status)) {
          const where = states[m.id]?.snapshot?.machine.machineName ?? m.name;
          system
            .notify(
              ev.status === "done" ? `Task done on ${where}` : `Task failed on ${where}`,
              (ev.status === "done" ? ev.result : ev.error)?.slice(0, 180) || ev.prompt.slice(0, 120),
            )
            .catch(() => {});
        }
        return;
      }
      if (m.isLocal) onLocalEvent(ev);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [me, onLocalEvent],
  );
  const agentsBusy = useMemo(
    () => machines.reduce((n, m) => n + (states[m.id]?.snapshot?.tasks ?? []).filter((t) => t.status === "queued" || t.status === "running").length, 0),
    [machines, states],
  );

  const readings = useMemo(
    () =>
      machines.map((m) => ({
        machine: states[m.id]?.snapshot?.machine.machineName ?? m.name,
        plan: states[m.id]?.snapshot?.plan ?? null,
      })),
    [machines, states],
  );
  const online = useMemo(
    () => machines.filter((m) => states[m.id]?.status === "online"),
    [machines, states],
  );
  const plan = usePlan(readings, now);
  usePlanAlert(plan, settings.planAlert);

  // Forget terminals for machines that were removed.
  useEffect(
    () =>
      setTerminals((t) => t.filter((id) => machines.some((m) => m.id === id))),
    [machines],
  );

  const openTerminal = (id: string) => {
    setTerminals((t) => (t.includes(id) ? t : [...t, id]));
    setView({ kind: "terminal", id });
  };
  const closeTerminal = (id: string) => {
    setTerminals((t) => t.filter((x) => x !== id));
    setView((v) =>
      v.kind === "terminal" && v.id === id ? { kind: "machine", id } : v,
    );
  };
  const sendTo = (m: MachineConfig, files: File[]) =>
    transfers.send(
      { ...m, name: states[m.id]?.snapshot?.machine.machineName ?? m.name },
      files,
    );

  const selected =
    view.kind === "machine"
      ? machines.find((m) => m.id === view.id)
      : undefined;
  const localMachine = machines.find((m) => m.isLocal);
  const viewKey =
    view.kind === "machine"
      ? `m:${view.id}`
      : view.kind === "terminal"
        ? "terminal"
        : view.kind;

  return (
    <div className="flex h-full flex-col">
      <Background />
      {machines.map((m) => (
        <Connection
          key={m.id}
          machine={m}
          onState={onState}
          onEvent={(ev) => onAnyEvent(m, ev)}
        />
      ))}
      {isMobile ? (
        <MobileHeader online={online.length} total={machines.length} />
      ) : (
        <TitleBar online={online.length} total={machines.length} />
      )}
      <div className="flex min-h-0 flex-1">
        {!isMobile && <Sidebar
          view={view}
          onView={setView}
          collapsed={settings.sidebarCollapsed}
          onToggle={() =>
            update({ sidebarCollapsed: !settings.sidebarCollapsed })
          }
          machines={machines}
          states={states}
          terminals={terminals}
          agentsBusy={agentsBusy}
        />}
        <main className={`thin-scroll flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto ${isMobile ? "gap-3 px-2.5 pb-3" : "gap-4 px-4 pb-4"}`}>
          <UpdateBanner updates={updates} />
          {incomingShells.length > 0 && (
            <div className="flex shrink-0 items-center gap-3 rounded-2xl bg-amber-500/15 px-4 py-2.5 text-[12.5px] text-amber-50 ring-1 ring-amber-300/30">
              <span className="h-2 w-2 shrink-0 rounded-full bg-amber-300" />
              <span className="min-w-0 flex-1">
                Remote shell open on this PC from{" "}
                {incomingShells.map((s) => `${s.from} (${s.ip})`).join(", ")}.
              </span>
              <button
                className="shrink-0 rounded-lg px-2 py-1 text-[11.5px] ring-1 ring-amber-200/30 hover:bg-amber-500/20"
                onClick={() => setView({ kind: "settings" })}
              >
                Settings
              </button>
            </div>
          )}
          {/* On a phone, room is scarce: plan usage only on the overview. */}
          {(isMobile ? view.kind === "overview" : view.kind !== "settings" && view.kind !== "terminal") && (
            <PlanBanner plan={plan} now={now} />
          )}
          {loaded && machines.length === 0 && view.kind !== "settings" && (
            <div className="glass rounded-3xl p-8 text-center text-white/60">
              {local?.error ? (
                `The local collector could not start: ${local.error}`
              ) : isMobile ? (
                <>
                  No PCs yet. On a PC, open Claude Fleet, go to Settings, This PC, <b>Copy code</b>, get the code to this phone, then add it here.
                  <button
                    className="mt-4 block w-full rounded-xl bg-violet-500/35 px-3 py-2.5 text-[13px] font-medium text-violet-50 ring-1 ring-violet-300/40"
                    onClick={() => setView({ kind: "settings" })}
                  >
                    Add a PC
                  </button>
                </>
              ) : (
                "No machines configured."
              )}
            </div>
          )}

          {/* Terminals stay mounted while you look at other pages, so their sessions stay open. */}
          {terminals.map((id) => {
            const m = machines.find((x) => x.id === id);
            return m ? (
              <Suspense key={id} fallback={null}>
                <TerminalView
                  key={id}
                  machine={m}
                  state={states[id]}
                  from={me}
                  visible={view.kind === "terminal" && view.id === id}
                  onClose={() => closeTerminal(id)}
                />
              </Suspense>
            ) : null;
          })}

          <AnimatePresence mode="wait">
            <motion.div
              key={viewKey}
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -4 }}
              transition={{ duration: 0.18 }}
              className="flex shrink-0 flex-col"
            >
              {view.kind === "overview" && (
                <Overview
                  machines={machines}
                  states={states}
                  now={now}
                  onOpen={(id) => setView({ kind: "machine", id })}
                />
              )}
              {view.kind === "agents" && (
                <AgentsView machines={machines} states={states} from={me} now={now} />
              )}
              {view.kind === "machine" && selected && (
                <MachineCard
                  machine={selected}
                  state={
                    states[selected.id] ?? {
                      status: "connecting",
                      snapshot: null,
                      lastMessageAt: null,
                      error: null,
                    }
                  }
                  now={now}
                  requestedBy={me}
                  onSendFiles={(files) => sendTo(selected, files)}
                  onOpenTerminal={() => openTerminal(selected.id)}
                  onOpenScreen={() => setScreenOf(selected.id)}
                />
              )}
              {view.kind === "settings" && (
                <SettingsView
                  updates={updates}
                  settings={settings}
                  update={update}
                  local={local}
                  localState={
                    localMachine ? states[localMachine.id] : undefined
                  }
                  saved={saved}
                  states={states}
                  onMachinesChange={async (list) => {
                    await updateSaved(list);
                    // Forget state for removed machines.
                    setStates((prev) =>
                      Object.fromEntries(
                        Object.entries(prev).filter(
                          ([id]) =>
                            id === "local" || list.some((m) => m.id === id),
                        ),
                      ),
                    );
                  }}
                />
              )}
            </motion.div>
          </AnimatePresence>
        </main>
      </div>
      {isMobile && (
        <MobileNav
          view={view}
          onView={setView}
          machines={machines}
          states={states}
          terminals={terminals}
          agentsBusy={agentsBusy}
        />
      )}
      {!isMobile && <DropOverlay
        machines={machines}
        states={states}
        onSend={sendTo}
        onReject={transfers.reject}
      />}
      <TransfersTray transfers={transfers} now={now} />
      {screenOf && machines.find((m) => m.id === screenOf) && (
        <ScreenView
          machine={machines.find((m) => m.id === screenOf)!}
          name={states[screenOf]?.snapshot?.machine.machineName ?? machines.find((m) => m.id === screenOf)!.name}
          from={me}
          now={now}
          onClose={() => setScreenOf(null)}
        />
      )}
    </div>
  );
}
