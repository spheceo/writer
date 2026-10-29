import { expect, test } from "bun:test";
import { TranscriptionBudget } from "./index";

/** A day's budget object with storage in memory. */
function budget(perPerson: string, total: string) {
  const kept = new Map<string, number>();
  const storage = {
    get: async <T>(key: string) => kept.get(key) as T | undefined,
    put: async (key: string, value: number) => void kept.set(key, value),
    transaction: async <T>(work: (inner: typeof storage) => Promise<T>) => work(storage),
    setAlarm: async () => {},
    deleteAll: async () => kept.clear(),
  };
  const env = { MAX_UPLOAD_BYTES: "45000000", DAILY_BYTES_PER_IP: perPerson, DAILY_BYTES_TOTAL: total, WATCH_DAILY_PER_PERSON: "3000", WATCH_DAILY_TOTAL: "12000" };
  return new TranscriptionBudget({ storage }, env as never);
}
const reserve = (object: TranscriptionBudget, client: string, size: number) =>
  object.fetch(new Request("https://budget/reserve", { method: "POST", body: JSON.stringify({ client: client.repeat(64), size }) }));

test("one person may use the whole shared day, and a refusal says which cap it hit", async () => {
  const day = budget("288000000", "288000000");
  expect((await reserve(day, "a", 40_000_000)).status).toBe(200);
  for (let i = 0; i < 6; i++) expect((await reserve(day, "a", 40_000_000)).status).toBe(200);
  const mine = await reserve(day, "a", 40_000_000);
  expect(mine.status).toBe(429);
  expect(await mine.json()).toEqual({ cap: "person", error: "Your daily transcription allowance (about 20 hours of audio per person) is used up; it resets at midnight UTC." });
  const theirs = await reserve(day, "b", 40_000_000);
  expect((await theirs.json()) as { cap: string; error: string }).toEqual({ cap: "team", error: "The team's shared daily transcription allowance (about 20 hours of audio across everyone) is used up; it resets at midnight UTC." });
});
