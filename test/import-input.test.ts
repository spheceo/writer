import { expect, test } from "bun:test";
import { pastedTrelloCard } from "../src/import-input";
test("a standalone Trello card fills the form; earlier-card references do not replace mixed cases", () => {
  expect(pastedTrelloCard("  https://trello.com/c/card1234/title\n")).toBe("https://trello.com/c/card1234/title");
  expect(pastedTrelloCard("REUSE https://youtu.be/aaaaaaaaaaa https://trello.com/c/card1234 case 4")).toBeNull();
  expect(pastedTrelloCard("https://drive.google.com/drive/folders/1folderfolderfolder\nREUSE https://youtu.be/aaaaaaaaaaa https://trello.com/c/card1234 case 4")).toBeNull();
});
