import { AnimatePresence, motion } from "framer-motion";
import { useState } from "react";
import { bytes, rate } from "../lib/format";
import { isMobile } from "../lib/machines";
import { system } from "../lib/system";
import type { Transfer, Transfers } from "../lib/transfers";

function Row({
  t,
  transfers,
  now,
}: {
  t: Transfer;
  transfers: Transfers;
  now: number;
}) {
  const [err, setErr] = useState<string | null>(null);
  const pct = t.size
    ? Math.min(100, (t.sent * 100) / t.size)
    : t.status === "done"
      ? 100
      : 0;
  const elapsed = ((t.finishedAt ?? now) - (t.startedAt ?? now)) / 1000;
  const speed =
    t.status === "sending" && elapsed > 0.5 ? t.sent / elapsed : null;
  const arrow = t.direction === "out" ? "↑" : "↓";

  let line: string;
  switch (t.status) {
    case "queued":
      line = `Waiting · to ${t.peer}`;
      break;
    case "sending":
      line = `${bytes(t.sent)} of ${bytes(t.size)}${speed ? ` · ${rate(speed)}` : ""} · to ${t.peer}`;
      break;
    case "done":
      line =
        t.direction === "out"
          ? `Sent to ${t.peer}${t.savedAs && t.savedAs !== t.name ? ` as ${t.savedAs}` : ""} · ${bytes(t.size)}`
          : `From ${t.peer}${t.savedAs && t.savedAs !== t.name ? ` · saved as ${t.savedAs}` : ""} · ${bytes(t.size)}`;
      break;
    case "cancelled":
      line = `Cancelled · to ${t.peer}`;
      break;
    default:
      line = t.error ?? "Failed";
  }

  return (
    <motion.li
      layout="position"
      initial={{ opacity: 0, y: 6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0 }}
      className="rounded-xl px-2.5 py-2 hover:bg-white/[0.04]"
    >
      <div className="flex items-center gap-2.5">
        <span
          className={`flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-[13px] font-semibold ${
            t.status === "failed"
              ? "bg-rose-500/20 text-rose-200"
              : t.direction === "out"
                ? "bg-violet-500/20 text-violet-100"
                : "bg-emerald-500/20 text-emerald-100"
          }`}
        >
          {arrow}
        </span>
        <div className="min-w-0 flex-1">
          <div className="truncate text-[12.5px] text-white/90" title={t.name}>
            {t.name}
          </div>
          <div
            className={`truncate text-[11px] tabular ${t.status === "failed" ? "text-rose-200/80" : "text-white/45"}`}
            title={err ?? line}
          >
            {err ?? line}
          </div>
        </div>
        {t.direction === "in" && t.path && system.available && (
          <button
            className="shrink-0 rounded-lg px-2 py-1 text-[11px] text-white/70 ring-1 ring-white/10 hover:bg-white/[0.08]"
            onClick={() =>
              system.revealReceived(t.path!).catch((e) => setErr(String(e)))
            }
          >
            Show
          </button>
        )}
        {t.status === "queued" || t.status === "sending" ? (
          <button
            className="shrink-0 rounded-lg px-1.5 py-1 text-white/50 hover:bg-white/[0.08] hover:text-white"
            title="Cancel"
            onClick={() => transfers.cancel(t.id)}
          >
            ✕
          </button>
        ) : (
          <button
            className="shrink-0 rounded-lg px-1.5 py-1 text-white/35 hover:bg-white/[0.08] hover:text-white"
            title="Dismiss"
            onClick={() => transfers.dismiss(t.id)}
          >
            ✕
          </button>
        )}
      </div>
      {t.status === "sending" && (
        <div className="mt-1.5 ml-[38px] h-1 overflow-hidden rounded-full bg-white/[0.08]">
          <div
            className="h-full rounded-full bg-gradient-to-r from-violet-300 to-sky-400 transition-[width] duration-200"
            style={{ width: `${pct}%` }}
          />
        </div>
      )}
    </motion.li>
  );
}

/** Bottom-right list of files being sent and files this PC received. */
export function TransfersTray({
  transfers,
  now,
}: {
  transfers: Transfers;
  now: number;
}) {
  const [open, setOpen] = useState(true);
  const items = transfers.items.slice(0, 8);
  const busy = transfers.items.filter(
    (t) => t.status === "queued" || t.status === "sending",
  ).length;
  if (!items.length) return null;

  return (
    // Outer wrapper does the positioning: `.glass` sets position: relative.
    <div className={isMobile ? "fixed inset-x-2 bottom-[84px] z-40" : "fixed right-4 bottom-4 z-40 w-[340px]"}>
      <div className="glass rounded-2xl p-2">
        <div className="flex items-center justify-between px-2 py-1">
          <button
            className="flex items-center gap-2 text-[12.5px] font-medium text-white/85"
            onClick={() => setOpen((o) => !o)}
          >
            <span
              className={`inline-block transition-transform ${open ? "" : "-rotate-90"}`}
            >
              ▾
            </span>
            Transfers{" "}
            {busy ? (
              <span className="text-white/45">· {busy} in progress</span>
            ) : null}
          </button>
          {!busy && (
            <button
              className="rounded-lg px-2 py-0.5 text-[11px] text-white/45 hover:bg-white/[0.06] hover:text-white"
              onClick={transfers.clearFinished}
            >
              Clear
            </button>
          )}
        </div>
        {open && (
          <ul className="thin-scroll mt-1 max-h-[340px] overflow-y-auto">
            <AnimatePresence initial={false}>
              {items.map((t) => (
                <Row key={t.id} t={t} transfers={transfers} now={now} />
              ))}
            </AnimatePresence>
          </ul>
        )}
      </div>
    </div>
  );
}
