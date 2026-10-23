import { expect, test } from "bun:test";
import { MANIFEST, manifestResponse } from "./manifest";

// Contract v2 (accounts/docs/plan.md): what accounts accepts from an app.
const ID = /^[a-z][a-z0-9-]*(\.[a-z][a-z0-9-]*)+$/;

test("the manifest keeps contract v2's rules", () => {
  expect(MANIFEST.app).toMatch(/^[a-z][a-z0-9-]{0,31}$/);
  expect(MANIFEST.version).toBe(1);
  const ids = MANIFEST.capabilities.map((capability) => capability.id);
  expect(new Set(ids).size).toBe(ids.length);
  expect(ids.length).toBeLessThanOrEqual(100);
  for (const capability of MANIFEST.capabilities) {
    expect(capability.id).toMatch(ID);
    expect(capability.id.length).toBeLessThanOrEqual(64);
    expect(capability.label.length).toBeLessThanOrEqual(60);
    expect(capability.description.length).toBeLessThanOrEqual(200);
  }
  expect(MANIFEST.presets.length).toBeLessThanOrEqual(20);
  for (const preset of MANIFEST.presets) for (const id of preset.capabilities) expect(ids).toContain(id);
  expect(MANIFEST.presets.find((preset) => preset.id === "full")?.capabilities).toEqual(ids);
  expect(MANIFEST.icon.startsWith("data:image/svg+xml;base64,")).toBe(true);
  expect(MANIFEST.icon.length).toBeLessThanOrEqual(16 * 1024);
  expect(atob(MANIFEST.icon.split(",")[1])).toContain("#ff0033");
});

test("it's served public and cacheable", async () => {
  const response = manifestResponse();
  expect(response.headers.get("access-control-allow-origin")).toBe("*");
  expect(response.headers.get("cache-control")).toContain("max-age=3600");
  expect((await response.json()).app).toBe("writer");
});
