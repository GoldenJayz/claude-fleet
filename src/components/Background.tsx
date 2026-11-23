/** A plain dark tint over the desktop; its strength is the Transparency setting. */
export function Background() {
  return <div className="window-tint pointer-events-none fixed inset-0 -z-10" />;
}
