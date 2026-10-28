import { expect, test } from "bun:test";
import { importError } from "../src/import-error";

test("saved transcription transport errors show recovery instructions without request URLs", () => {
  const message = importError("The connection to Writer's service broke (https://relay/v1/transcribe?keyterm=Jane+Doe: Network Error: peer closed connection without sending TLS close_notify). Retry: what was heard is kept.");
  expect(message).toContain("saved parts");
  expect(message).toContain("Logs on the Mac that ran the import");
  expect(message).not.toContain("https://");
  expect(message).not.toContain("Jane");
});

test("other import failures keep their explanation", () => {
  expect(importError("ElevenLabs has no credit left for this.")).toBe("ElevenLabs has no credit left for this.");
  expect(importError(null)).toBe("It didn't finish.");
});
