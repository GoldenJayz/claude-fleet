import { AnimatePresence, motion } from 'framer-motion';
import { useEffect, useRef, useState } from 'react';
import type { CollectorState } from '../lib/collector';
import type { MachineConfig } from '../types';

const isFileDrag = (e: DragEvent) => !!e.dataTransfer && Array.from(e.dataTransfer.types).includes('Files');

/** Split a drop into files we can send and folders we can't (yet). */
export function filesFromDrop(dt: DataTransfer): { files: File[]; folders: string[] } {
  const files: File[] = [];
  const folders: string[] = [];
  for (const item of Array.from(dt.items)) {
    if (item.kind !== 'file') continue;
    const entry = item.webkitGetAsEntry?.();
    const file = item.getAsFile();
    if (entry?.isDirectory) folders.push(entry.name);
    else if (file) files.push(file);
  }
  return { files, folders };
}

export const FOLDER_HINT = 'Folders can’t be sent yet. Zip it first (right-click → Compress to ZIP file) and send the .zip.';

function why(m: MachineConfig, st?: CollectorState): string | null {
  if (m.isLocal) return 'this PC';
  if (st?.status !== 'online') return st?.status === 'unauthorized' ? 'token rejected' : 'offline';
  const features = st.snapshot?.machine.features ?? [];
  if (!features.includes('files')) return 'update Pane there first';
  return null;
}

/**
 * Drag files anywhere over the window: every PC appears as a drop target.
 * Dropping on one sends the files to its "Downloads\Pane" folder.
 */
export function DropOverlay({
  machines,
  states,
  onSend,
  onReject,
}: {
  machines: MachineConfig[];
  states: Record<string, CollectorState>;
  onSend: (m: MachineConfig, files: File[]) => void;
  onReject: (machineName: string, name: string, error: string) => void;
}) {
  const [show, setShow] = useState(false);
  const [hover, setHover] = useState<string | null>(null);
  const depth = useRef(0);

  useEffect(() => {
    const enter = (e: DragEvent) => {
      if (!isFileDrag(e)) return;
      e.preventDefault();
      depth.current++;
      setShow(true);
    };
    const leave = (e: DragEvent) => {
      if (!isFileDrag(e)) return;
      depth.current = Math.max(0, depth.current - 1);
      if (depth.current === 0) {
        setShow(false);
        setHover(null);
      }
    };
    // Without this, dropping outside a target would make the window navigate to the file.
    const over = (e: DragEvent) => {
      if (isFileDrag(e)) e.preventDefault();
    };
    const drop = (e: DragEvent) => {
      e.preventDefault();
      depth.current = 0;
      setShow(false);
      setHover(null);
    };
    window.addEventListener('dragenter', enter);
    window.addEventListener('dragleave', leave);
    window.addEventListener('dragover', over);
    window.addEventListener('drop', drop);
    return () => {
      window.removeEventListener('dragenter', enter);
      window.removeEventListener('dragleave', leave);
      window.removeEventListener('dragover', over);
      window.removeEventListener('drop', drop);
    };
  }, []);

  const remotes = machines.filter((m) => !m.isLocal);

  return (
    <AnimatePresence>
      {show && (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          className="fixed inset-0 z-50 flex flex-col items-center justify-center gap-6 bg-black/55 p-10"
        >
          <div className="text-center">
            <div className="font-display text-2xl font-semibold">Send to…</div>
            <div className="mt-1 text-[13px] text-white/55">Drop on a PC. Files go to its Downloads\Pane folder.</div>
          </div>
          {remotes.length === 0 ? (
            <div className="glass max-w-md rounded-3xl p-6 text-center text-[13px] text-white/65">
              No other PCs yet. Add one in Settings → Machines, then drag files here to send them.
            </div>
          ) : (
            <div className="flex max-w-5xl flex-wrap justify-center gap-4">
              {remotes.map((m) => {
                const st = states[m.id];
                const blocked = why(m, st);
                const name = st?.snapshot?.machine.machineName ?? m.name;
                const active = hover === m.id && !blocked;
                return (
                  <div
                    key={m.id}
                    onDragEnter={() => setHover(m.id)}
                    onDragOver={(e) => {
                      e.preventDefault();
                      e.dataTransfer.dropEffect = blocked ? 'none' : 'copy';
                      if (hover !== m.id) setHover(m.id);
                    }}
                    onDragLeave={(e) => {
                      if (!e.currentTarget.contains(e.relatedTarget as Node)) setHover((h) => (h === m.id ? null : h));
                    }}
                    onDrop={(e) => {
                      e.preventDefault();
                      if (blocked) return;
                      const { files, folders } = filesFromDrop(e.dataTransfer);
                      folders.forEach((f) => onReject(name, f, FOLDER_HINT));
                      if (files.length) onSend({ ...m, name }, files);
                    }}
                    className={`flex h-44 w-64 flex-col items-center justify-center gap-3 rounded-3xl border-2 border-dashed transition-colors ${
                      blocked
                        ? 'border-white/10 bg-white/[0.03] text-white/35'
                        : active
                          ? 'border-violet-300/80 bg-violet-500/25 text-white'
                          : 'border-white/25 bg-white/[0.06] text-white/85'
                    }`}
                  >
                    <svg width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" className="pointer-events-none">
                      <rect x="2.5" y="4" width="19" height="12.5" rx="2" />
                      <path d="M8 20.5h8M12 16.5v4" />
                      {!blocked && <path d="M12 7v6M9.5 10.5L12 13l2.5-2.5" />}
                    </svg>
                    <div className="pointer-events-none text-center">
                      <div className="font-display text-lg font-semibold">{name}</div>
                      <div className="text-[12px] text-white/50">{blocked ?? m.address}</div>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );
}
