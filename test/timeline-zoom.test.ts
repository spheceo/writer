import { expect, test } from "bun:test";
import { resizeTimelineWindow } from "../src/timeline-zoom";
test("overview edges zoom while preserving the opposite boundary", () => {
  expect(resizeTimelineWindow(1000, 9000, 2000, "in", 10000)).toEqual({ start: 3000, end: 9000 });
  expect(resizeTimelineWindow(1000, 9000, -3000, "out", 10000)).toEqual({ start: 1000, end: 6000 });
});
test("zoom cannot invert the window or leave the sequence", () => {
  expect(resizeTimelineWindow(1000, 9000, 100000, "in", 10000)).toEqual({ start: 8800, end: 9000 });
  expect(resizeTimelineWindow(1000, 9000, -100000, "in", 10000).start).toBe(0);
  expect(resizeTimelineWindow(1000, 9000, 100000, "out", 10000).end).toBe(10000);
});
