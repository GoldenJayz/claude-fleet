import { AnimatePresence, motion } from 'framer-motion';
import { useCallback, useEffect, useMemo, useState } from 'react';
import { Background } from './components/Background';
import { MachineCard } from './components/MachineCard';
import { Overview } from './components/Overview';
import { PlanBanner } from './components/PlanBanner';
import { SettingsView } from './components/SettingsView';
import { Sidebar, type View } from './components/Sidebar';
import { TitleBar } from './components/TitleBar';
import { useCollector, useNow, type CollectorState } from './lib/collector';
import { useMachines } from './lib/machines';
import { usePlan } from './lib/plan';
import { usePlanAlert } from './lib/alerts';
import { useUiSettings } from './lib/settings';
import type { MachineConfig } from './types';

/** Keeps one live connection per machine regardless of which view is showing. */
function Connection({ machine, onState }: { machine: MachineConfig; onState: (id: string, s: CollectorState) => void }) {
  const state = useCollector(machine);
  useEffect(() => onState(machine.id, state), [machine.id, state, onState]);
  return null;
}

export default function App() {
  const { machines, loaded, local, saved, updateSaved } = useMachines();
  const [settings, update] = useUiSettings();
  const now = useNow(1000);
  const [states, setStates] = useState<Record<string, CollectorState>>({});
  const [view, setView] = useState<View>({ kind: 'overview' });

  const onState = useCallback((id: string, s: CollectorState) => {
    setStates((prev) => (prev[id] === s ? prev : { ...prev, [id]: s }));
  }, []);

  const readings = useMemo(
    () => machines.map((m) => ({ machine: states[m.id]?.snapshot?.machine.machineName ?? m.name, plan: states[m.id]?.snapshot?.plan ?? null })),
    [machines, states],
  );
  const online = useMemo(() => machines.filter((m) => states[m.id]?.status === 'online'), [machines, states]);
  const plan = usePlan(readings, now);
  usePlanAlert(plan, settings.planAlert);

  const selected = view.kind === 'machine' ? machines.find((m) => m.id === view.id) : undefined;
  const localMachine = machines.find((m) => m.isLocal);
  const viewKey = view.kind === 'machine' ? `m:${view.id}` : view.kind;

  return (
    <div className="flex h-full flex-col">
      <Background />
      {machines.map((m) => (
        <Connection key={m.id} machine={m} onState={onState} />
      ))}
      <TitleBar online={online.length} total={machines.length} />
      <div className="flex min-h-0 flex-1">
        <Sidebar
          view={view}
          onView={setView}
          collapsed={settings.sidebarCollapsed}
          onToggle={() => update({ sidebarCollapsed: !settings.sidebarCollapsed })}
          machines={machines}
          states={states}
        />
        <main className="thin-scroll flex min-h-0 min-w-0 flex-1 flex-col gap-4 overflow-y-auto px-4 pb-4">
          {view.kind !== 'settings' && <PlanBanner plan={plan} now={now} />}
          {loaded && machines.length === 0 && (
            <div className="glass rounded-3xl p-8 text-center text-white/60">
              {local?.error ? `The local collector could not start: ${local.error}` : 'No machines configured.'}
            </div>
          )}
          <AnimatePresence mode="wait">
            <motion.div
              key={viewKey}
              initial={{ opacity: 0, y: 6 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -4 }}
              transition={{ duration: 0.18 }}
              className="flex shrink-0 flex-col"
            >
              {view.kind === 'overview' && (
                <Overview machines={machines} states={states} now={now} onOpen={(id) => setView({ kind: 'machine', id })} />
              )}
              {view.kind === 'machine' && selected && (
                <MachineCard
                  machine={selected}
                  state={states[selected.id] ?? { status: 'connecting', snapshot: null, lastMessageAt: null, error: null }}
                  now={now}
                  requestedBy={local?.machineName ?? 'dashboard'}
                />
              )}
              {view.kind === 'settings' && (
                <SettingsView
                  settings={settings}
                  update={update}
                  local={local}
                  localState={localMachine ? states[localMachine.id] : undefined}
                  saved={saved}
                  states={states}
                  onMachinesChange={async (list) => {
                    await updateSaved(list);
                    // Forget state for removed machines.
                    setStates((prev) => Object.fromEntries(Object.entries(prev).filter(([id]) => id === 'local' || list.some((m) => m.id === id))));
                  }}
                />
              )}
            </motion.div>
          </AnimatePresence>
        </main>
      </div>
    </div>
  );
}
