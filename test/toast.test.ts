import { afterEach, expect, jest, test } from "bun:test";
import { startToastTimer, TOAST_MS } from "../src/ui";

const savedWindow = globalThis.window;

afterEach(() => {
  jest.useRealTimers();
  globalThis.window = savedWindow;
});

function fakeWindow() {
  jest.useFakeTimers();
  globalThis.window = globalThis as unknown as Window & typeof globalThis;
}

test("each toast leaves after five seconds, and a later toast leaves the earlier timer running", () => {
  fakeWindow();
  const gone: string[] = [];
  const stopFirst = startToastTimer(() => gone.push("first"));
  jest.advanceTimersByTime(TOAST_MS - 1);
  const stopSecond = startToastTimer(() => gone.push("second"));
  expect(gone).toEqual([]);
  jest.advanceTimersByTime(1);
  expect(gone).toEqual(["first"]);
  jest.advanceTimersByTime(TOAST_MS - 1);
  expect(gone).toEqual(["first", "second"]);
  stopFirst();
  stopSecond();
});

test("clearing a toast timer on unmount keeps it from firing", () => {
  fakeWindow();
  const gone: string[] = [];
  const stop = startToastTimer(() => gone.push("gone"));
  stop();
  jest.advanceTimersByTime(TOAST_MS);
  expect(gone).toEqual([]);
});
