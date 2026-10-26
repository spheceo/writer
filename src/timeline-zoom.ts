/** Resize the overview window while holding the opposite edge in place. */
export function resizeTimelineWindow(start: number, end: number, deltaMs: number, edge: "in" | "out", total: number, minSpan = 200) {
  const span = Math.min(total, minSpan);
  return edge === "in"
    ? { start: Math.max(0, Math.min(end - span, start + deltaMs)), end }
    : { start, end: Math.min(total, Math.max(start + span, end + deltaMs)) };
}
