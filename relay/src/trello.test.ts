import { expect, test } from "bun:test";
import { attachmentLabels } from "./index";

test("Trello keeps case and reuse labels alongside all attachment URLs", () => {
  expect(attachmentLabels([
    { name: "Case 1: 751) Teacher - Google Drive", url: "https://drive.google.com/drive/folders/1folderfolderfolder" },
    { name: "Case 3 (reuse): Finished script", url: "https://docs.google.com/document/d/1docdocdocdocdoc/edit" },
    { name: "Case 4\nTeacher", url: "https://youtu.be/aaaaaaaaaaa" },
    { name: "No URL" },
  ])).toEqual([
    "Case 1: 751) Teacher - Google Drive <https://drive.google.com/drive/folders/1folderfolderfolder>",
    "Case 3 (reuse): Finished script <https://docs.google.com/document/d/1docdocdocdocdoc/edit>",
    "Case 4 Teacher <https://youtu.be/aaaaaaaaaaa>",
  ]);
});
