import type { AgentMode, AgentStatus, AgentStep, AgentTask, MachineConfig } from '../types';
import { httpBase } from './collector';

/** Agent tasks: hand a prompt to Claude Code on one of your machines. */

async function req<T>(m: MachineConfig, path: string, init?: RequestInit): Promise<T> {
  const r = await fetch(`${httpBase(m)}${path}`, {
    ...init,
    headers: { Authorization: `Bearer ${m.token}`, 'Content-Type': 'application/json' },
  });
  if (!r.ok) {
    let msg = `HTTP ${r.status}`;
    try {
      msg = ((await r.json()) as { error?: string }).error ?? msg;
    } catch {
      /* keep HTTP status */
    }
    throw new Error(msg);
  }
  return r.json() as Promise<T>;
}

export const agentStatus = (m: MachineConfig) => req<AgentStatus>(m, '/agent');
export const getTask = (m: MachineConfig, id: number) => req<AgentTask>(m, `/agent/tasks/${id}`);
export const cancelTask = (m: MachineConfig, id: number) => req<{ ok: boolean }>(m, `/agent/tasks/${id}/cancel`, { method: 'POST' });

export interface TaskRequest {
  prompt: string;
  cwd: string;
  mode: AgentMode;
  model?: string;
  effort?: string;
  /** Queue this as the next message in that task's conversation. */
  followUpOf?: number;
  from: string;
}

export const submitTask = (m: MachineConfig, body: TaskRequest) => req<AgentTask>(m, '/agent/tasks', { method: 'POST', body: JSON.stringify(body) });

export const MODES: { value: AgentMode; label: string; hint: string }[] = [
  { value: 'plan', label: 'Read-only', hint: 'Looks around and answers or plans; changes nothing.' },
  { value: 'acceptEdits', label: 'Edit files', hint: 'Can edit files in the folder; commands that need approval are skipped.' },
  { value: 'auto', label: 'Auto', hint: 'Claude Code’s auto mode: runs what its safety checks allow.' },
  { value: 'bypassPermissions', label: 'Full access', hint: 'Runs anything without asking. Needs “full access” on that PC.' },
];

export const MODELS = [
  { value: '', label: 'Default model' },
  { value: 'opus', label: 'Opus' },
  { value: 'sonnet', label: 'Sonnet' },
  { value: 'haiku', label: 'Haiku' },
];

// Live steps for open task views, fed by the machines' streams.
type StepListener = (machineId: string, taskId: number, step: AgentStep) => void;
const listeners = new Set<StepListener>();
export function onTaskStep(f: StepListener) {
  listeners.add(f);
  return () => {
    listeners.delete(f);
  };
}
export function publishTaskStep(machineId: string, taskId: number, step: AgentStep) {
  listeners.forEach((f) => f(machineId, taskId, step));
}

export const isActive = (t: AgentTask) => t.status === 'queued' || t.status === 'running';

export function folderName(path: string) {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || path;
}
