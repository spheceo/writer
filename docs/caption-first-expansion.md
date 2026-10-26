# Caption-first expansion for compilation sources

The existing local courtroom caption/turn implementation is now expanded in the
working tree to every YouTube recording, including Interrogation, Bodycam and
compilations. A one-new-video-plus-reuse compilation resolves its first part's
YouTube URL correctly. Drive/file recordings without YouTube captions still use
the existing cached speech hearing; a cached hearing wins over a fresh fetch.

Mixed-source compilations take each recording's caption or hearing separately,
then move its words onto the compilation clock. A final word is clamped at the
recording's boundary. Caption provenance remains `captions.json` / model
`youtube-captions`, not a falsely labeled Scribe transcript shared with Edit.

The existing `turns.rs` subject/speaker pass now describes police footage,
interrogations, hearings, calls, narration and separate compilation cases instead
of assuming every recording is one court hearing. It accepts mixed caption turns
and already-heard voices, keeps the evidence requirement for names, and retains
the existing cached answers and bounded window processing. A separate person's
case is not automatically the same person in the next recording.

Verification: combined-tree `just check` passed 58 JavaScript tests and 200 Rust
tests (10 optional integrations ignored). New regressions cover all three styles,
the one-new-case-plus-reuse URL mapping, mixed YouTube/Drive inputs and word ends
at recording joins. No paid calls, deployment, release or restart was performed.

Commit boundary: `captions.rs`, `turns.rs` and their model/import dependencies
were already uncommitted work from another task. This expansion does not absorb
that unrelated dependency chain into its own commit. The precise added hunks are
saved at `/tmp/writer-caption-first-request.patch`, relative to those starting
files; they are already applied to the main working tree. Commit that dependency
work together through its owning thread before calling this a self-contained
Writer build. This note can be committed independently.
