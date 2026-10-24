import { expect, test } from "bun:test";
import { CAN, grantsOf, workOf } from "../src/can";

test("names each kind of script as the app does", () => {
  expect(workOf({ sourceKind: "youtube", compilation: false, kind: "interrogation" })).toBe(CAN.interrogation);
  expect(workOf({ sourceKind: "drive", compilation: false, kind: "foia" })).toBe(CAN.foia);
  expect(workOf({ sourceKind: "compilation", compilation: false, kind: "courtroom" })).toBe(CAN.courtroom);
  expect(workOf({ sourceKind: "compilation", compilation: true, kind: "foia" })).toBe(CAN.foiaCompilation);
  expect(workOf({ sourceKind: "compilation", compilation: true, kind: "courtroom" })).toBe(CAN.courtroomCompilation);
  expect(workOf({ sourceKind: "commentary", compilation: false, kind: undefined })).toBe(CAN.commentary);
});

test("offers only what was granted; admins everything, bodycam compilations included", () => {
  const writer = grantsOf(["use", CAN.interrogationCompilation, "cloud"], false);
  expect(writer.singleKinds).toEqual([]);
  expect(writer.compilationKinds).toEqual(["interrogation"]);
  expect(writer.has(CAN.share)).toBe(false);
  const admin = grantsOf([], true);
  expect(admin.singleKinds).toEqual(["interrogation", "foia", "courtroom"]);
  expect(admin.compilationKinds).toEqual(["interrogation", "foia", "courtroom"]);
});
