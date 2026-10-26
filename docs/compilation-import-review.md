# Compilation imports, reuse and player review

Ready for code review; no deployment, release, tag or PR was published.

## Main integration

The compilation/player work is now committed on `main` in the normal Writer
checkout, with no PR. The temporary feature worktree and branch were removed.
Existing unrelated Brief, courtroom and packaging changes in the main checkout
were preserved as uncommitted work; verification ran against the combined tree.

Agent review caught and fixed distinct numbered cases sharing a recording being
deduplicated, missing attachment case slots, and trailing footage/thumbnail
references becoming cases. Canonical URL lines retain distinct cases and their
timestamp hints. A folder master already registered under another compilation's
filename now keeps that actual registry key in serialized `script.json`, including
later saves from an older in-memory script. These fixes have offline regressions.

Final combined verification passed `just check` (58 JavaScript tests, 198 Rust
tests; 10 optional integrations ignored), frontend production build, explicit
relay typecheck, and both selected real-ffmpeg folder-master integrations. Relay
auth/transport regressions also passed independently (5 tests, 53 assertions).

## Changes

- `src-tauri/src/trello.rs`: the list picker and pasted card use the same supported-source extraction. Drive folders survive alongside YouTube and script Docs; Markdown labels, reuse annotations, attachment labels, attachments, checklists and comments are read. Description cases lead; additional unique sources follow. A numbered new case clears the previous case's reuse annotation.
- `src-tauri/src/import.rs`: mixed labeled paste and canonical lines use that parser. One folder is one case, with every name-ordered recording downloaded and retained; a saved manifest freezes retry order. A full-size case master feeds the 480p preview. One new case plus a reuse is valid; an unsupported source produces an error rather than disappearing.
- `src-tauri/src/reuse.rs`, `model.rs`, `src/types.ts`: preserve full original script text separately from the shortened transition context. Direct Docs retain their full fetched text; earlier compilation cases retain the selected original section, timestamps, link targets and finished cut-in words. The build receives only transition context and never rewrites this saved script evidence.
- `src-tauri/src/media.rs`, `handoff.rs`, `transcribe.rs`: retain a folder master's resolution and expose its registry identity in JSON. A compilation with only one new case hands off its original/master instead of its preview.
- `src/import-input.ts`, `library.tsx`: a standalone Trello URL fills the form; an earlier-card reference within a mixed/reuse paste does not replace it. Single-line mixed sources work, and checked rows retain case numbering through reuse rows.
- `src/captions.ts`, `monitor.tsx`, `editor.tsx`, `styles.css`: port Repack's source-timed caption grouping, corrections, trim filtering, sentence/speaker/pause breaks and picture-relative sizing. Subtitles default on, including Split; CC toggles them. Cut-ins and source audition have no subtitles.
- `src/timeline.tsx`, `timeline-zoom.ts`: draggable overview ends zoom into a portion while preserving the opposite edge; the middle pans, double-click fits, and existing buttons/pinch zoom remain available. Scroll anchoring applies after layout.
- `relay/src/index.ts`, `auth.ts`: keep attachment names alongside URLs. Only `/v1/trello/card` and `/v1/trello/cards` accept Edit tokens: admin or `use` plus `edit.*`, with existing JWT verification and per-person limits. Handlers remain GET-only; other Writer routes retain their audience restrictions, including the existing Feedback Docs exception.

## Import/handoff contract shared with Edit

Canonical ordered lines:

```text
https://drive.google.com/drive/folders/<folder-id>
https://youtu.be/<new-video-id>
REUSE https://docs.google.com/document/d/<script-id>/edit
https://youtu.be/<next-video-id>
REUSE https://youtu.be/<reused-video-id> https://trello.com/c/<earlier-card> case 4
```

Docs always reuse an existing script. `(reuse)`, `[reuse]`, `REUSE`, and equivalent case/section annotations are accepted. Existing-script reuse preserves footage ranges and cut-in words; a video supplied explicitly as reuse remains an explicit video source for Edit to reuse.

`reuses[].after` counts **new cases** preceding the reuse, not all case rows. `reuses[].url` is the input source. `script` is the full Google Doc URL, not an ID; `scriptTitle` is its title. `scriptText` is **plain text**, not HTML: the complete text returned by Writer's existing Doc reader, or the exact selected earlier-case section. Inline link targets appear as `<https://…>`; ranges and finished words remain intact. The Doc reader already normalizes HTML formatting/whitespace; `scriptText` adds no further cleaning or truncation. `narration` remains the separate, shortened transition context.

`clips[].reuse` and `parts[].reuse` retain the matching reuse URL. They and `reuses[].script/scriptTitle/scriptText` survive Script JSON serialization, storage and cloud handoff. A composite folder part additionally has `mediaSource: "sha256:<master-hash>"` and `originalKey: "edit/originals/sha256/<master-hash>/00.mp4"` (the filename reflects the source index). Its `url` remains the original folder URL. Edit must use this registry master for that case clock rather than selecting a folder's first file.

## Verification

Passed:

```sh
bun run build
just check
node_modules/.bin/tsc --noEmit --skipLibCheck --target ES2022 --module ESNext --moduleResolution bundler --lib ES2022,DOM relay/src/index.ts
cargo test --manifest-path src-tauri/Cargo.toml folder_master -- --ignored
```

`just check`: 58 JavaScript tests; 176 Rust tests; 10 optional integration tests ignored. Both explicitly selected ffmpeg integration tests passed: full-resolution multi-recording master and exact serialized handoff identity/original selection.

Regression coverage includes the four-case screenshot layout (synthetic URLs), list versus pasted-card extraction, labeled attachments, reuse annotation isolation, mixed/canonical round trips, one new case plus a Doc, unsupported input rejection, untouched long scripts and selected earlier sections, reuse JSON round trips, subtitle source jumps/corrections/trims/gaps, actual Monitor rendering over footage versus cut-ins/audition, overview bounds, audience/grant/write-route rejection and the real relay handler with a mocked Trello response.

## Desktop/integration review

1. In New script → Compilation, paste the four-case card or its labeled description. Check Drive folder, YouTube, reused Doc, YouTube in that order. Repeat via Import from Trello; try a case supplied only as a labeled attachment. Check an earlier-card reference does not replace a mixed paste.
2. Import a multi-recording folder case. Verify all recordings play in name order as one case. Confirm `parts[].url`, `mediaSource` and `originalKey` in `script.json`; compare the registry master key when reviewing Edit's matching handoff change.
3. Compare a reused script's ranges and finished cut-in words with `reuses[].scriptText`. The compilation may add its transition; Edit must reconstruct the saved script without regenerating those supplied words. Test a long script and an earlier compilation's selected case.
4. Play Split view; seek across edits and a cut-in, and verify corrected subtitles follow the picture and disappear over the cut-in. Toggle CC. In Video view drag the overview's ends, pan its middle, use zoom buttons/pinch, scrub the zoomed ruler, and double-click to fit.
5. After an approved relay deployment, verify Edit's normal short-lived bearer can GET a card/list, while a subtitle-only account cannot. No additional Trello secrets are needed in Edit.

Authenticated production Drive/Docs/Trello downloads and live desktop pointer/video interaction were not exercised here. Attachment-name transport and Edit-audience Trello access require the reviewed relay change to be deployed later. The corresponding Edit parser/rebuild/handoff is owned by the parent thread. Repack was inspected read-only; its shared-parser issue was reported to its separate thread.
