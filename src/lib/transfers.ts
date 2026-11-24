import { useCallback, useEffect, useRef, useState } from 'react';
import type { MachineConfig, ReceivedFile } from '../types';
import { uploadFile } from './collector';

export type TransferStatus = 'queued' | 'sending' | 'done' | 'failed' | 'cancelled';

export interface Transfer {
  id: string;
  direction: 'out' | 'in';
  /** The other PC: where it's going, or where it came from. */
  peer: string;
  name: string;
  size: number;
  sent: number;
  status: TransferStatus;
  error?: string;
  startedAt?: number;
  finishedAt?: number;
  /** Name it was saved under on the receiving PC (may get a " (2)" suffix). */
  savedAs?: string;
  /** Full path, for files received on this PC. */
  path?: string;
}

interface Job {
  id: string;
  machine: MachineConfig;
  file: File;
}

let seq = 0;
const nextId = () => `t${Date.now().toString(36)}${(seq++).toString(36)}`;

/**
 * Files being sent from this dashboard (one at a time, in order) and files received by this PC.
 * Finished entries stay listed until dismissed.
 */
export function useTransfers(from: string) {
  const [items, setItems] = useState<Transfer[]>([]);
  const queue = useRef<Job[]>([]);
  const active = useRef<{ id: string; abort: () => void } | null>(null);
  const fromRef = useRef(from);
  fromRef.current = from;

  const patch = useCallback((id: string, p: Partial<Transfer>) => setItems((list) => list.map((t) => (t.id === id ? { ...t, ...p } : t))), []);

  const pump = useCallback(() => {
    if (active.current) return;
    const job = queue.current.shift();
    if (!job) return;
    const startedAt = Date.now();
    patch(job.id, { status: 'sending', startedAt });
    let last = 0;
    const up = uploadFile(job.machine, job.file, fromRef.current, (sent) => {
      // Progress events can fire very often; ~8 updates a second is plenty.
      const now = performance.now();
      if (now - last > 120) {
        last = now;
        patch(job.id, { sent });
      }
    });
    active.current = { id: job.id, abort: up.abort };
    up.promise
      .then((r) => patch(job.id, { status: 'done', sent: job.file.size, savedAs: r.name, finishedAt: Date.now() }))
      .catch((e: Error & { cancelled?: boolean }) =>
        patch(job.id, { status: e.cancelled ? 'cancelled' : 'failed', error: e.cancelled ? undefined : e.message, finishedAt: Date.now() }),
      )
      .finally(() => {
        active.current = null;
        pump();
      });
  }, [patch]);

  const send = useCallback(
    (machine: MachineConfig, files: File[]) => {
      const jobs = files.map((file) => ({ id: nextId(), machine, file }));
      setItems((list) => [
        ...jobs.map((j) => ({ id: j.id, direction: 'out' as const, peer: machine.name, name: j.file.name, size: j.file.size, sent: 0, status: 'queued' as const })),
        ...list,
      ]);
      queue.current.push(...jobs);
      // Let React add the rows before the first progress update.
      setTimeout(pump, 0);
    },
    [pump],
  );

  /** Something that can't be sent (e.g. a folder); shown as a failed row with the reason. */
  const reject = useCallback((machineName: string, name: string, error: string) => {
    setItems((list) => [{ id: nextId(), direction: 'out', peer: machineName, name, size: 0, sent: 0, status: 'failed', error, finishedAt: Date.now() }, ...list]);
  }, []);

  const cancel = useCallback(
    (id: string) => {
      if (active.current?.id === id) active.current.abort();
      else {
        queue.current = queue.current.filter((j) => j.id !== id);
        patch(id, { status: 'cancelled', finishedAt: Date.now() });
      }
    },
    [patch],
  );

  const received = useCallback((r: ReceivedFile) => {
    setItems((list) => [
      { id: `in${r.id}-${r.ts}`, direction: 'in', peer: r.from, name: r.sentName, savedAs: r.name, path: r.path, size: r.size, sent: r.size, status: 'done', finishedAt: r.ts },
      ...list,
    ]);
  }, []);

  const dismiss = useCallback((id: string) => setItems((list) => list.filter((t) => t.id !== id)), []);
  const clearFinished = useCallback(() => setItems((list) => list.filter((t) => t.status === 'queued' || t.status === 'sending')), []);

  // Abort an in-flight upload if the app is closed/reloaded.
  useEffect(() => () => active.current?.abort(), []);

  return { items, send, reject, cancel, received, dismiss, clearFinished };
}

export type Transfers = ReturnType<typeof useTransfers>;
