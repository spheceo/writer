# Writer

A desktop script editor. Footage is cut on this machine. Every key (ElevenLabs, Repack's Google Drive account, the YouTube proxy) lives on a Cloudflare relay, so none sits in the app and nobody installing Writer needs one. Scripts are written the way Repack writes them: Opus reads the whole transcript with Repack's cut-in guide and judgment calls (`src-tauri/guides/`), sends one plan of footage ranges and cut-in paragraphs, and the app checks it and cleans it up as Repack does (`src-tauri/src/tidy.rs`): breathing room at clip edges, edges that run on into fillers or stop mid-sentence moved to where the thought ends, and pauses cut to a beat only where the footage's sound is quiet (a gap holding speech the transcript missed stays). A compilation outside 6 to 7 Letter pages (counted as Repack counts them, `pages.rs`) or a FOIA cut under ten minutes goes back to Opus to bring in. It runs through the Claude Code login already on this Mac, so a build is paid by the Claude plan, not API calls. **Copy script** gives the text in Repack's Doc layout.

Every Repack format is covered, with Repack's own guides (`src-tauri/guides/`) and wording (`src-tauri/src/formats.rs`): **Interrogation**, **FOIA** (one-run or two-split opening, a dated case opening, a 10-minute floor), **Courtroom** (a two-paragraph opening, pacing that relaxes in the second half), and **compilations** of 2 to 20 videos, one case each, in either style (a title that opens with "When" is a standard compilation, any other a hidden one). The format is picked when building.

**New script** takes what Repack's form takes: an optional **article** about the case (read when the import starts, in place of searching); a compilation's videos **checked before anything downloads** (private, gone, live, age-restricted, over 8 hours), with **REUSE** before a link for a case the channel already covered (not imported: it takes only its transition, whose link goes into the Doc as a comment; **Reused cases** in Claude's panel can import its whole video later, joined onto the footage without encoding the rest again, transcribed alone, and read by Opus for its 5 to 8 strongest moments, which the build may open the video on as intro clips and nowhere else, `reuse.rs`); a compilation's case given as the **Google Doc of a script the channel already made** (always a reused case: nothing in it is rewritten; Writer reads the Doc for its title, the footage under Assets, which **Reused cases** can bring in, and its narration, which the transition is written to lead into, `reuse.rs`); several videos as **one Courtroom case** (a link's `t` kept as a hint to where the case starts); and a Drive case folder's **PDFs** (up to 20), which Haiku reads through Claude Code's Read tool for the case's facts (`casefiles.rs`). An incident report among them stands in for the web search.

**Import from Trello** in New script lists the cards waiting on Repack's lists, read through the relay (which holds Repack's Trello key; nothing is written back) and parsed as Repack parses them (`trello.rs`): **Raw Channels** ("Script (Sphe)": a case a card, its first Drive folder or video as the footage, its first other web page as the article, its name as the title; a card labelled "Missing footage" is passed over) and **Compilations** ("Ready for script (comp)": the YouTube, Drive and script Doc links from "case N" onwards, a link reused when its line or the line above says so). A card a script already has the name of is hidden. Picking one fills the dialog, and a compilation's links are checked at once.

Once a script is built, a message to Claude **revises** it, as Repack's chat editor does: Sonnet answers with targeted edits (trim, take out or insert words, split, move, delete, add or rewrite a cut-in, fix a spelling, tighten pauses, fix edges), which Writer checks and applies all or none (`revise.rs`), with a repair turn for any it refuses. A rewrite of narration already saved is a **proposal**, shown as a word diff in the Script view to accept or decline. The conversation is kept with the script and goes back to Claude as context. Rebuilding from scratch is still there, behind a confirmation.

**Commentary** scripts are the other kind: an investigative video essay in the Internet Janitor's style (`src-tauri/guides/commentary.md`), written from sources instead of one case's footage. **New script → Commentary** takes a title and, optionally, links; the first link is the brief. In the project, **Add** brings in more: a Google Doc, a Drive text file or a web page (shared with "Anyone with the link", or with Repack's Drive account), a text file on this Mac, pasted text, or the transcript of footage already in Writer. Each source is the **brief** (the angle and the evidence to use), **evidence** (what the script quotes), or **style** (a past script, for the voice; never quoted). Opus reads them all in full and writes the montage, the intro, numbered chapters and the ending, with every clip naming its recording and times, editor notes, and a list of checks before recording. Writer refuses a quote its source doesn't hold, a link no source gives, a clip cited from a style example, and a timed transcript's clip whose times don't match the words. Each project has a **pacing** for its builds: **Channel** (the channel's own clip-led videos: a line of setup, then the person talks) or **Documentary** (narration-led like Turkey Tom's: about three quarters narration, a short clip about every 30 to 40 seconds, and footage shown under the voice as "Show:" notes). With Documentary pacing, Writer sends a plan back when its narration share or clip count is far off. Narration, editor notes and paragraphs can be edited or removed; clips stay as checked. **Copy script** gives the channel's Doc layout, with a "Total time = …min vo + … = …min" line (narration at 150 words a minute), the recordings with their links, the sources and the checks.

**Feedback** teaches Claude what a reviewer marked on a commentary's Doc, and tests that it learned it (`src-tauri/src/feedback.rs`, `src/feedback-panel.tsx`). **Pull** reads the open comments people left (Drive's list, with the words each is on; Writer's own clip notes are left out) and the highlights (from the Doc's HTML; a highlight under a comment is that comment), and finds each in the script. Any item can be left out, and a highlight without a comment can be given a note saying what's wrong. **Run round** then:

1. Makes the answer key (first round only): Opus applies the feedback directly.
2. Writes the lesson (when there's none): Opus turns the feedback into a skill in general terms. A lesson that copies six words in a row from the script, or uses a name only the script has, is sent back; it would show the round the answers.
3. Tests it: a fresh Opus gets the sources, the script and the lesson, never the feedback, and is told only that a reviewer found problems of those kinds.
4. Grades the round: Opus compares what changed, paragraph by paragraph, with the feedback and the key. Each item is fixed, partly, missed or wrong, and every change nobody asked for is marked harmless or harmful. The match counts fixed as 1 and partly as a half.

**Improve the lesson** has Opus rewrite it from the last round's misses, or the producer edits it by hand; then run another round. **Keep the lesson** adds it to the lessons every commentary build reads, after the guide (kept in `lessons.json` beside `scripts/` and synced as `writer/lessons.json` on R2: every Mac merges its lessons with the cloud's by ID, and a lesson forgotten on one Mac is forgotten on all). **Use this round's script** makes a round's script the script, with what it didn't fix applied directly. Pulling again starts over. Everything else is under the script's `feedback/` folder, so it syncs with the script. A round takes about 15 minutes; the first, which also makes the key and the lesson, takes longer.

**Review** shares a script on a Repack link (`src-tauri/src/reviews.rs`, `src/review.tsx`): anyone with it reads the script and leaves notes under a name, no account. The page shows the script as blocks whose IDs stay as it is edited (`blocks.rs`): each footage clip (its range and words), each cut-in's narration, a commentary's montage, headings and paragraphs. **Share again** publishes the next version on the same link. While Writer runs it asks Repack for changed notes every 45 seconds; scripts with open notes get a count on their row and on the Review button. Two minutes after a reviewer's last note (so one sitting is fixed together), Claude takes the open notes (`reviews:claim`, so no note is fixed twice, on any Mac) and fixes them one at a time (`fixing.rs`): a footage script with the chat's edits, saved narration written straight in; a commentary with edits to the paragraphs the note is about, so nothing else can change. What each note changed is measured word by word on the blocks and sent back with Claude's summary; the page and Writer's Script view show it as green bands, and hovering one shows the reviewer's words, their name, Claude's summary and the version it lands in. A note Claude made no change for gets its reason as a reply and stays open; one a reviewer opens again is fixed again. The Review dialog turns the automatic fixing off, or fixes the open notes now. **Approve vN** marks the latest version done: the link takes no more notes, nothing is fixed by itself, and Repack deletes the other versions' review files (every note from every round stays; Writer's own version history is separate and untouched). An approval made on Repack's page is heard on the poll (`reviews:open`); the row and the Review button show "Approved vN". Sharing again starts the next version, unapproved; an approval can be taken back.

**edit: notes** are for the video editor: asked for one ("add an edit note: a blue waveform for this 911 call"), Claude puts it on that footage clip's timestamp, or under a commentary paragraph's editor note, starting `edit:`. They show in purple beside the page, go into the Google Doc, sync with the script, and Edit's agent follows them in that section when the script is handed over. On a written commentary, a message now changes only the paragraphs it is about; **Rewrite the whole script** under the box writes it again from the sources.

**Clips → Find clips** searches YouTube for what a commentary brief asks for (`src-tauri/src/scout.rs`). Opus turns the brief into up to 30 moments the video needs, each with a few searches; yt-dlp runs them and Sonnet picks up to three likely videos a moment (the original stream or podcast first), and the brief's own YouTube links are always read. yt-dlp fetches each video's English captions, timed to the word, without the video; Sonnet reads them for the moments and copies their words, and Writer keeps only those whose words are in the captions near their times. Where a moment's point is what's on screen, Writer downloads just those seconds at 360p, tiles 12 frames into one image, and Sonnet says whether it shows what's wanted. Each video with a moment goes in as timed evidence holding its captions around each moment, so the next build can play its clips; a second search replaces those sources. The Clips tab lists them by the brief's chapters, with each video's link at the moment, the words, why it fits and the frames. A search takes 10 to 20 minutes; videos without English captions are passed over.

Before writing, a build:

- **Names the speakers** with Haiku through the Claude CLI, as Repack's speaker check does: "Name (Role)" only when a full name is stated, otherwise the role. It also runs right after transcription.

Right after transcription, every import also **checks who speaks** by watching the footage, as Repack's and Edit's watcher does: Gemini (through the relay) sees and hears about four minutes at a time, four stretches at once, and says who says each line and who each person is. Speech to text tells voices apart by sound alone, so this splits two detectives it heard as one and joins one person it heard as two; a guess changes nothing. Each person gets "Name (Role)" when the footage gives a full name, otherwise the role; then Haiku reads the corrected transcript for the suspects, the voices the footage didn't show, and a full name the title or transcript gives someone the footage only gave a role. Each stretch's answer is kept, so a retry doesn't pay twice (about $0.50 an hour of footage). Audio has nothing to watch, and a failure is a note on the script: the speakers stay as heard and Haiku names them as before.
- **Researches the case for free**, the way the sibling Edit app does: Haiku writes a few searches, Writer reads DuckDuckGo's plain HTML results and the pages on this Mac's own connection (no search API or key), and Sonnet keeps only the reports about this case with their facts (dates, places, names, charges, outcomes). They go to the builder as RESEARCH. **Research again** in the script looks once more. A failed search never stops a build.

**Google Doc** exports a script as Repack does (`apps/web/src/server/script-documents.ts`): a new Doc each time, made from a Word file Writer lays out (`src/doc-export.ts`, `src/docx.ts`) so Docs keeps its comments, in the "Writer scripts" folder of the account that owns Repack's script Docs, open to anyone with the link as an editor. It lists the footage under Assets (a compilation's or a Courtroom case's videos in order), and every timestamp links to its own video's moment where there is one and carries a comment (a footage script's first and last words; a commentary clip's quote and what to verify), narration words to blur get Repack's "Note to editor: Blur this" (a commentary's get "Censor", and its editor notes become comments), all signed GeniusScriptwriter10. **Export draft** makes the same Doc named "(DRAFT) …" without replacing the script's Doc; the dialog says when the script has changed since the last export, lists every export (from Drive, trashed ones marked), and reads back the comments people left on the latest Doc. A commentary can be rewritten with the open ones. The relay makes the Docs; their account's access never leaves it.

Editing has undo and redo (⌘Z, ⇧⌘Z), 30 steps, including undoing a fresh build. Edits show at once and save in turn; one refused because the script changed elsewhere takes the script from disk. **Versions** keeps every save (one author's edits within ten minutes merged), says who made each and what changed, and restores one. Clips move by dragging on the timeline; transcript words go in at the playhead as an insert edit, or at the start, and a word can be respelled. A Courtroom script's Script view drops the blank lines between blocks after the first, as Repack's does (the Doc runs them on in one paragraph). The heading can be rewritten in the Script view, and a comment written on a timestamp there goes into the Doc on that timestamp. A build's outcome is kept beside the script, so one that ended unwatched, or one Writer closed on, says so.

Titles and thumbnails are not in this slice. Scripts live in Application Support and sync through the cloud.

## What a user installs

Only **Claude Code**, signed in (run `claude` once). Everything else comes with Writer or is fetched by it:

- **ffmpeg and ffprobe** are bundled inside the packaged app.
- **yt-dlp** is Writer's own copy: the official macOS build from yt-dlp's GitHub releases, checked against the release's SHA-256 list, unpacked in Application Support and replaced when a month old.
- **HTTP** is Writer's own, so there is no curl, and no Infisical or Google key on the user's Mac.

An app opened from the Dock doesn't see a terminal's PATH, so Writer looks for each program itself: its own copy, the bundled one, then Homebrew, `~/.local/bin` and the system folders. Settings shows each one and where it came from.

## Run

```sh
bun install
just dev      # development: uses ffmpeg from Homebrew
just bundle   # the app to hand out, with ffmpeg and ffprobe inside
```

`just bundle` fetches static ffmpeg and ffprobe once (`scripts/fetch-ffmpeg.sh`, Martin Riedl's macOS builds linked from ffmpeg.org) into `src-tauri/binaries/`, refusing any that need a library macOS doesn't ship.

Footage comes from three places, as in Repack:

- **YouTube.** `yt-dlp` downloads on this Mac's own connection, which costs nothing. The relay's DataImpulse proxy (Repack's `PROXY_URL`) takes over only for a busy hour (4 or more YouTube downloads in the last hour, as a 20-video compilation would be) or when YouTube blocks this Mac; each try gets a fresh sticky exit. The proxy's login reaches the Mac only for the length of a download.
- **Google Drive.** A file link, or a folder link for a case: every video and audio recording in the folder and its subfolders, in name order, joined into one timeline as Repack joins them. Writer reads them as the Drive account Repack imports with: the relay holds that account's sign-in and hands Writer a one-hour access token. Share footage with that account. Downloads come straight from Google.
- **This Mac.** The file stays where it is.
- **A compilation.** 2 to 12 YouTube or Drive video links, one case each, joined in order with a "Video N" header per case.

Work runs side by side: up to 3 imports at once, 3 Drive recordings downloading at once, and 4 transcription pieces heard at once (the relay allows 60 requests a minute per Mac, as Edit's does).

When an import goes wrong it stops in a state you can act on, as in Repack:

- **Retry** carries on from where it stopped. The YouTube download (its `.part` too), each Drive recording, the preview and each transcribed piece are kept, so nothing downloads or is paid for twice. An import that stopped because Writer closed, or that hit a bug, is retried the same way.
- **Needs your choice** pauses an import on a question instead of failing it: some of a folder's recordings didn't come through (go on without them, retry them, or cancel); a folder holds more than an import takes (60 recordings, 20 hours, 8 hours or 30 GB a file: choose which); or no words were heard (open it without a transcript).
- A recording cut short (an interrupted upload) imports what it holds, and the script says so. One that downloads short is fetched again once from where it stopped, then dropped so a retry starts it clean.
- A YouTube bot check or dropped connection with nothing downloaded yet gets one more try, on a fresh proxy exit when there is one.
- Transcription retries only what can't have been billed: the relay unreachable, or busy (after 30 s, then 60 s). Refusals (no credit, a wrong token) say what the relay said and stop.
- Claude busy or overloaded gets one more try a minute later; a plan it sends wrong, or no plan, gets up to two repair turns on the same session. Nothing is saved to the script until a plan passes.
- Settings checks the setup: Claude Code signed in, ffmpeg and ffprobe, Writer's yt-dlp, and the relay with transcription, Drive and the proxy.

Every import makes a 480p preview (a joined folder's footage is that preview) and is transcribed through the relay.

`just dev` listens on every interface, so the same page is at `http://kenobi:1420/` (or `kenobi.tail863066.ts.net:1420`) from another machine on the tailnet. The Writer window must stay open on this Mac, since it serves the API that Vite proxies; port 1421 itself only listens on localhost. Opening a file from that browser takes a path on this Mac. The file dialog only runs in the Writer window.

Builds and imports run on this Mac, not in the browser. A build is a background job the page checks every few seconds, so a dropped connection, a sleeping laptop or a reload does not lose it, and the open script is in the URL. Over plain `http` the browser has no Clipboard API; **Copy script** falls back to the older copy command.

## Relay

`relay/` holds Writer's keys. Everything but `/health` needs the client token:

- `POST /v1/transcribe` forwards Opus audio to ElevenLabs Scribe v2 with Edit's parameters, the case's names as `keyterm`s. It caps bursts and a day's audio per IP and across everyone (~20 hours, as Edit's). Byte caps stand in for duration; they are not a hard dollar cap.
- `GET /v1/drive/token` gives a one-hour access token for Repack's Drive account. The refresh token stays on the relay.
- `GET /v1/proxy` gives the DataImpulse proxy for YouTube downloads.
- `POST /v1/watch` sends a few minutes of footage to Gemini 3.8 Flash through the Vercel AI Gateway (the relay picks the model; 40 a minute per IP).
- `POST /v1/docs/export`, `GET /v1/docs/exports` and `GET /v1/docs/comments` make a script's Docs, list them and read their comments, as the account that owns Repack's script Docs. Only Docs in its "Writer scripts" folder marked with the script are listed or read.
- `POST /v1/cloud/*` is the cloud, as Edit's relay has it: presigned R2 URLs for keys under `writer/` only, and calls to Convex functions named `writer:` only, with Convex's service token added. It refuses everything when no client token is set (`relay/src/cloud.ts`).

```sh
cd relay
bunx wrangler deploy
bunx wrangler secret put CLIENT_TOKEN          # users paste it into Settings
bunx wrangler secret put ELEVENLABS_API_KEY
# Repack's, copied from its Infisical (/media, dev) without printing them:
for k in GOOGLE_DRIVE_CLIENT_ID GOOGLE_DRIVE_CLIENT_SECRET GOOGLE_DRIVE_REFRESH_TOKEN PROXY_URL; do
  infisical secrets get $k --projectId cdf4a0c5-eca4-4415-af3a-40871af285f3 --path=/media --env=dev --plain \
    | tr -d '\n' | bunx wrangler secret put $k
done
# The cloud: Repack's R2 bucket and Convex deployment (shared with Edit; Writer keeps to writer/):
for pair in /web:R2_ACCOUNT_ID:R2_ACCOUNT_ID /web:R2_ACCESS_KEY_ID:R2_ACCESS_KEY_ID /web:R2_SECRET_ACCESS_KEY:R2_SECRET_ACCESS_KEY \
  /web:R2_BUCKET:R2_BUCKET /web:NEXT_PUBLIC_CONVEX_URL:CONVEX_URL /convex:CONVEX_SERVICE_TOKEN:CONVEX_SERVICE_TOKEN; do
  scope=${pair%%:*}; rest=${pair#*:}   # not "path": in zsh that is PATH
  infisical secrets get ${rest%%:*} --projectId cdf4a0c5-eca4-4415-af3a-40871af285f3 --path=$scope --env=dev --plain \
    | tr -d '\n' | bunx wrangler secret put ${rest#*:}
done
# The script Docs account (Repack's GOOGLE_DOCS_REFRESH_TOKEN and its client, at /) and the AI Gateway:
for pair in GOOGLE_DOCS_CLIENT_ID:GOOGLE_DRIVE_CLIENT_ID GOOGLE_DOCS_CLIENT_SECRET:GOOGLE_DRIVE_CLIENT_SECRET \
  GOOGLE_DOCS_REFRESH_TOKEN:GOOGLE_DOCS_REFRESH_TOKEN AI_GATEWAY_API_KEY:AI_GATEWAY_API_KEY; do
  infisical secrets get ${pair#*:} --projectId cdf4a0c5-eca4-4415-af3a-40871af285f3 --path=/ --env=dev --plain \
    | tr -d '\n' | bunx wrangler secret put ${pair%%:*}
done
```

Anyone with the client token can read that Drive account for an hour at a time and use the proxy, so treat the token like a password. The app looks for `https://writer-relay.sphe-g-personal.workers.dev` until you change that URL.

Footage is heard as Edit hears it, one system for both apps (`hearing.rs`, a port of Edit's `transcribe.ts`): a recording up to 90 s in one go; a longer one twice at once, in 40 s pieces (4 s either side) for its words and times, and whole for who speaks, the two lined up by what they say. A joined case is heard recording by recording, from each one's own file (joined cases now keep their recordings). Every request listens for the case's names (`names.rs`, Edit's `names.ts`: Gemini lists the people and places its articles, case files and narration give, each word checked against them, surnames on their own too); case PDFs are read before the footage is heard, so their names count. What's heard is kept as it comes in, so a retry never pays twice; each recording's transcript is `transcript/<recording>/transcript.json` in Edit's format.

**The handoff to Edit** (`handoff.rs`): after an import, and on **Send to Edit**, each recording goes to Edit's footage registry (`edit:putMedia`, under `drive:<id>`, `youtube:<id>` or `sha256:<hex>`) with its original on R2 under `edit/originals/`, and its words to `edit/analysis/<source>/transcript.json`; before hearing a recording, Writer reads Edit's transcript of it if there is one. A recording heard before Writer heard footage Edit's way is heard again with the case's names first, so Edit never needs to. What is in Edit's originals (`edit-shared.json`) leaves Writer's own sync. YouTube videos are downloaded in Edit's format (H.264 up to 1080p), so both apps share one file. Send to Edit marks the script on its Convex row (`writer:setHandoff`) for Edit's home screen.

## Cloud

Every script is in the cloud and on every Mac running Writer, as Edit's projects are. Each script's body (`script.json`) and files (the proxy, the original, transcript pieces, sources, research, the watcher's answers) sit on Cloudflare R2 under `writer/scripts/<id>/` in Repack's bucket; Convex (Repack's deployment, `apps/web/convex/writer.ts` in the Repack repo, published with Repack's `db:push`) keeps the index: each script's rev, body and files. Writer only talks to the relay.

`src-tauri/src/cloud.rs` runs in the background whenever Writer has its relay token. Saves land on disk as always; a few seconds after a script is still, its changed files go up, then a new body, then `writer:commitScript` naming the rev it began from. Every 30 seconds it pulls what other Macs pushed. A commit another Mac overtook is refused, and this Mac's version is kept as "<title> (conflicted copy)" before the cloud's is taken. Scripts still importing or being written by Claude wait until they are done. Deleting a script marks it deleted in Convex (so every Mac removes it) and deletes its files from R2. What this Mac knows is in `cloud/ledger.json` beside `scripts/`; deletions waiting for the connection in `cloud/deleted.json`. The lists and editors show the state (synced, uploading, offline), and an open script follows changes that come down.

The bodies are on R2, not in Convex's file storage: Edit's nightly sweep deletes Convex-stored JSON that no Edit project uses. Keep Writer's keys under `writer/`: Repack's own cleanup deletes unrecorded files under `repack/`.
