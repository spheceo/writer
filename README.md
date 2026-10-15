# Writer

A desktop script editor. Footage is cut on this machine. Every key (ElevenLabs, Repack's Google Drive account, the YouTube proxy) lives on a Cloudflare relay, so none sits in the app and nobody installing Writer needs one. Scripts are written the way Repack writes them: Opus reads the whole transcript with Repack's cut-in guide and judgment calls (`src-tauri/guides/`), sends one plan of footage ranges and cut-in paragraphs, and the app checks it, adds breathing room at clip edges and tightens long pauses. It runs through the Claude Code login already on this Mac, so a build is paid by the Claude plan, not API calls. **Copy script** gives the text in Repack's Doc layout.

Every Repack format is covered, with Repack's own guides (`src-tauri/guides/`) and wording (`src-tauri/src/formats.rs`): **Interrogation**, **FOIA** (one-run or two-split opening, a dated case opening, a 10-minute floor), **Courtroom** (a two-paragraph opening, pacing that relaxes in the second half), and **compilations** of 2 to 12 videos, one case each, in either style (a title that opens with "When" is a standard compilation, any other a hidden one). The format is picked when building. Follow-up chat edits and reused cases are not covered.

**Commentary** scripts are the other kind: an investigative video essay in the Internet Janitor's style (`src-tauri/guides/commentary.md`), written from sources instead of one case's footage. **New script → Commentary** takes a title and, optionally, links; the first link is the brief. In the project, **Add** brings in more: a Google Doc, a Drive text file or a web page (shared with "Anyone with the link", or with Repack's Drive account), a text file on this Mac, pasted text, or the transcript of footage already in Writer. Each source is the **brief** (the angle and the evidence to use), **evidence** (what the script quotes), or **style** (a past script, for the voice; never quoted). Opus reads them all in full and writes the montage, the intro, numbered chapters and the ending, with every clip naming its recording and times, editor notes, and a list of checks before recording. Writer refuses a quote its source doesn't hold, a link no source gives, a clip cited from a style example, and a timed transcript's clip whose times don't match the words. Narration, editor notes and paragraphs can be edited or removed; clips stay as checked. **Copy script** gives the channel's Doc layout, with a "Total time = …min vo + … = …min" line (narration at 150 words a minute), the recordings with their links, the sources and the checks.

Before writing, a build:

- **Names the speakers** with Haiku through the Claude CLI, as Repack's speaker check does: "Name (Role)" only when a full name is stated, otherwise the role. It also runs right after transcription.

Right after transcription, every import also **checks who speaks** by watching the footage, as Repack's and Edit's watcher does: Gemini (through the relay) sees and hears about four minutes at a time, four stretches at once, and says who says each line and who each person is. Speech to text tells voices apart by sound alone, so this splits two detectives it heard as one and joins one person it heard as two; a guess changes nothing. Each person gets "Name (Role)" when the footage gives a full name, otherwise the role; then Haiku reads the corrected transcript for the suspects, the voices the footage didn't show, and a full name the title or transcript gives someone the footage only gave a role. Each stretch's answer is kept, so a retry doesn't pay twice (about $0.50 an hour of footage). Audio has nothing to watch, and a failure is a note on the script: the speakers stay as heard and Haiku names them as before.
- **Researches the case for free**, the way the sibling Edit app does: Haiku writes a few searches, Writer reads DuckDuckGo's plain HTML results and the pages on this Mac's own connection (no search API or key), and Sonnet keeps only the reports about this case with their facts (dates, places, names, charges, outcomes). They go to the builder as RESEARCH. **Research again** in the script looks once more. A failed search never stops a build.

**Google Doc** exports a script as Repack does (`apps/web/src/server/script-documents.ts`): a new Doc each time, made from a Word file Writer lays out (`src/doc-export.ts`, `src/docx.ts`) so Docs keeps its comments, in the "Writer scripts" folder of the account that owns Repack's script Docs, open to anyone with the link as an editor. Every timestamp links to its moment where there is one and carries a comment (a footage script's first and last words; a commentary clip's quote and what to verify), narration words to blur get Repack's "Note to editor: Blur this" (a commentary's get "Censor", and its editor notes become comments), all signed GeniusScriptwriter10. **Export draft** makes the same Doc named "(DRAFT) …" without replacing the script's Doc; the dialog says when the script has changed since the last export, lists every export (from Drive, trashed ones marked), and reads back the comments people left on the latest Doc. A commentary can be rewritten with the open ones. The relay makes the Docs; their account's access never leaves it.

Editing has undo and redo (⌘Z, ⇧⌘Z), 30 steps, including undoing a fresh build.

Convex sync, titles, and thumbnails are not in this slice. Scripts live in Application Support.

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

Work runs side by side: up to 3 imports at once, 3 Drive recordings downloading at once, and 3 transcription pieces heard at once (the relay allows 12 requests a minute per Mac).

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

- `POST /v1/transcribe` forwards Opus audio to ElevenLabs Scribe v2. It caps bursts and a day's audio per IP and across everyone. Byte caps stand in for duration; they are not a hard dollar cap.
- `GET /v1/drive/token` gives a one-hour access token for Repack's Drive account. The refresh token stays on the relay.
- `GET /v1/proxy` gives the DataImpulse proxy for YouTube downloads.
- `POST /v1/watch` sends a few minutes of footage to Gemini 3.8 Flash through the Vercel AI Gateway (the relay picks the model; 40 a minute per IP).
- `POST /v1/docs/export`, `GET /v1/docs/exports` and `GET /v1/docs/comments` make a script's Docs, list them and read their comments, as the account that owns Repack's script Docs. Only Docs in its "Writer scripts" folder marked with the script are listed or read.

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
# The script Docs account (Repack's GOOGLE_DOCS_REFRESH_TOKEN and its client, at /) and the AI Gateway:
for pair in GOOGLE_DOCS_CLIENT_ID:GOOGLE_DRIVE_CLIENT_ID GOOGLE_DOCS_CLIENT_SECRET:GOOGLE_DRIVE_CLIENT_SECRET \
  GOOGLE_DOCS_REFRESH_TOKEN:GOOGLE_DOCS_REFRESH_TOKEN AI_GATEWAY_API_KEY:AI_GATEWAY_API_KEY; do
  infisical secrets get ${pair#*:} --projectId cdf4a0c5-eca4-4415-af3a-40871af285f3 --path=/ --env=dev --plain \
    | tr -d '\n' | bunx wrangler secret put ${pair%%:*}
done
```

Anyone with the client token can read that Drive account for an hour at a time and use the proxy, so treat the token like a password. The app looks for `https://writer-relay.sphe-g-personal.workers.dev` until you change that URL.

Long recordings are sent in pieces of about two hours, split near a quiet moment. A piece that already succeeded is kept, so a retry does not pay ElevenLabs again.
