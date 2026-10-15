import { open as pickFile } from "@tauri-apps/plugin-dialog";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { addSource, buildStatus, getScript, inTauri, listScripts, message, removeSource, saveDraft, sourceText, startBuild, updateSource } from "./api";
import { clock } from "./clips";
import { commentaryDocument, cue, narrationWords, timing } from "./commentary";
import type { Adding, BuildStatus, Cite, Draft, Paragraph, Role, Script, Source, Summary } from "./types";
import { DocsDialog } from "./docs-dialog";
import { Dialog, Icon, Menu, Tabs } from "./ui";

const ROLES: { value: Role; label: string; hint: string }[] = [
  { value: "brief", label: "Brief", hint: "The producer's brief: the angle and the evidence to use" },
  { value: "evidence", label: "Evidence", hint: "Reports, transcripts and pages the script quotes from" },
  { value: "style", label: "Style", hint: "A past script, for the voice and shape. Never quoted" },
];

const size = (chars: number) => (chars >= 1_000_000 ? `${(chars / 1e6).toFixed(1)}M` : chars >= 1000 ? `${Math.round(chars / 1000)}k` : `${chars}`);

/** About four characters a token: what Opus reads, for a rough sense of size. */
const tokens = (chars: number) => Math.round(chars / 4);

export function CommentaryEditor({
  script,
  onChange,
  onBack,
}: {
  script: Script;
  onChange: (script: Script) => void;
  onBack: () => void;
}) {
  const sources = script.commentary?.sources ?? [];
  const draft = script.commentary?.draft ?? null;
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState<"saving" | "saved" | null>(null);
  const [copied, setCopied] = useState(false);
  const [adding, setAdding] = useState(false);
  const [reading, setReading] = useState<Source | null>(null);
  const [started, setStarted] = useState<number | null>(null);
  const [stage, setStage] = useState("");
  const [report, setReport] = useState<{ text: string; costUsd: number } | null>(null);
  const [rewriting, setRewriting] = useState(false);
  const [docsOpen, setDocsOpen] = useState(false);
  // Comments read back from the Doc, waiting in a rewrite's notes.
  const [noteSeed, setNoteSeed] = useState("");
  const latest = useRef(script);
  latest.current = script;

  // A build runs on the Mac; the page asks after it every few seconds.
  const alive = useRef(true);
  const watching = useRef(false);
  const follow = (since: number) => {
    setStarted(since);
    if (watching.current) return;
    watching.current = true;
    const poll = async () => {
      if (!alive.current) {
        watching.current = false;
        return;
      }
      let job: BuildStatus | null;
      try {
        job = await buildStatus(script.id);
      } catch {
        window.setTimeout(poll, 5000);
        return;
      }
      if (job?.running) {
        setStage(job.stage);
        window.setTimeout(poll, 3000);
        return;
      }
      watching.current = false;
      setStarted(null);
      if (!job) return;
      if (job.error) {
        setError(job.error);
        return;
      }
      setRewriting(false);
      try {
        onChange(await getScript(script.id));
      } catch (cause) {
        setError(message(cause));
      }
      setReport({ text: job.report ?? "", costUsd: job.costUsd ?? 0 });
    };
    window.setTimeout(poll, 3000);
  };

  useEffect(() => {
    alive.current = true;
    void buildStatus(script.id)
      .then((job) => job?.running && follow(job.startedAt))
      .catch(() => {});
    return () => {
      alive.current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [script.id]);

  const build = async (note: string) => {
    setReport(null);
    setError(null);
    setStage("Starting");
    try {
      const job = await startBuild(script.id, note);
      follow(job.startedAt);
      return true;
    } catch (cause) {
      setError(message(cause));
      return false;
    }
  };

  const persist = async (next: Draft) => {
    setSaving("saving");
    try {
      onChange(await saveDraft(script.id, latest.current.revision, next));
      setSaving("saved");
      window.setTimeout(() => setSaving((now) => (now === "saved" ? null : now)), 1200);
    } catch (cause) {
      setSaving(null);
      setError(message(cause));
    }
  };

  const change = async (work: Promise<Script>) => {
    setError(null);
    try {
      onChange(await work);
    } catch (cause) {
      setError(message(cause));
    }
  };

  const copy = async () => {
    if (!draft) return;
    const text = commentaryDocument(script.title, draft, sources);
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Plain http on the tailnet has no Clipboard API; the old command works from a click.
      const area = window.document.createElement("textarea");
      area.value = text;
      area.readOnly = true;
      area.style.cssText = "position:fixed;top:0;left:0;opacity:0;pointer-events:none";
      window.document.body.append(area);
      area.select();
      const done = window.document.execCommand("copy");
      area.remove();
      if (!done) {
        setError("This browser would not copy the script. Open Writer on the Mac, or select the text by hand.");
        return;
      }
    }
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  const busy = started !== null;
  const ready = sources.some((source) => source.role !== "style");
  const total = sources.reduce((sum, source) => sum + source.chars, 0);
  const times = draft ? timing(draft) : null;

  const editParagraph = (section: number, index: number, next: Paragraph | null) => {
    if (!draft) return;
    const sections = draft.sections.map((item, at) =>
      at !== section
        ? item
        : { ...item, paragraphs: next ? item.paragraphs.map((p, i) => (i === index ? next : p)) : item.paragraphs.filter((_, i) => i !== index) },
    );
    void persist({ ...draft, sections });
  };

  return (
    <div className="app">
      <header className="top" data-tauri-drag-region>
        <button type="button" className="ghost sm" onClick={onBack}>← Library</button>
        <h1 data-tauri-drag-region>{script.title}</h1>
        <span className="spacer" data-tauri-drag-region />
        {saving && <span className={saving === "saved" ? "ok" : "muted"}>{saving === "saved" ? "Saved" : "Saving…"}</span>}
        <button type="button" className="sm" disabled={!draft} title="Export to Google Docs, see earlier exports and read comments" onClick={() => setDocsOpen(true)}>
          Google Doc
        </button>
        <button
          type="button"
          className={draft ? "primary sm" : "sm"}
          disabled={!draft}
          title="Copies the script in the channel's Doc layout: sections, clips inline, editor notes, total time, sources and checks"
          onClick={() => void copy()}
        >
          {copied ? "Copied" : "Copy script"}
        </button>
      </header>
      {error && (
        <div className="notice">
          {error}
          <button type="button" className="ghost sm" onClick={() => setError(null)}>Dismiss</button>
        </div>
      )}
      <div className="editor commentary">
        <section className="panel">
          <div className="panel-head">
            <h2>Sources</h2>
            <span className="faint">
              {sources.length} · about {size(tokens(total))} tokens
            </span>
            <span className="spacer" />
            <button type="button" className="sm" disabled={busy} onClick={() => setAdding(true)}>
              <Icon name="plus" size={14} />
              Add
            </button>
          </div>
          <div className="sources">
            {sources.length === 0 && (
              <div className="empty">
                <p>No sources yet.</p>
                <p className="hint">Add the producer's brief, the evidence, and a past script or two for the style.</p>
              </div>
            )}
            {sources.map((source) => (
              <div className="source-row" key={source.id}>
                <span className="badge mono">{source.id}</span>
                <button type="button" className="open" title="Read it" onClick={() => setReading(source)}>
                  <strong>{source.title}</strong>
                  <span className="meta">
                    {size(source.chars)} characters{source.timed ? " · timed transcript" : ""} · {origin(source)}
                  </span>
                </button>
                <select
                  value={source.role}
                  disabled={busy}
                  title={ROLES.find((role) => role.value === source.role)?.hint}
                  onChange={(event) => void change(updateSource(script.id, source.id, { role: event.target.value as Role }))}
                >
                  {ROLES.map((role) => (
                    <option key={role.value} value={role.value}>{role.label}</option>
                  ))}
                </select>
                <Menu
                  label={`More for ${source.title}`}
                  items={[
                    { label: "Read", onSelect: () => setReading(source) },
                    ...(source.origin.startsWith("http") ? [{ label: "Open link", onSelect: () => window.open(source.origin, "_blank") }] : []),
                    { label: "Remove", danger: true, disabled: busy, onSelect: () => void change(removeSource(script.id, source.id)) },
                  ]}
                />
              </div>
            ))}
          </div>
          <div className="builder-dock">
            {(!draft || rewriting || busy) && (
              <Builder
                key={noteSeed}
                ready={ready}
                started={started}
                stage={stage}
                rewrite={!!draft}
                seed={noteSeed}
                onBuild={(note) => build(note).then((done) => (done && setNoteSeed(""), done))}
                onCancel={draft ? () => setRewriting(false) : undefined}
              />
            )}
          </div>
        </section>

        <section className="panel">
          <div className="panel-head">
            <h2>Script</h2>
            {draft && times && (
              <span className="faint">
                {clock(times.totalMs)} · {narrationWords(draft)} words of narration · {clock(times.clipsMs)} of clips
              </span>
            )}
            <span className="spacer" />
            {draft && !rewriting && !busy && (
              <button type="button" className="ghost sm" onClick={() => setRewriting(true)}>Rewrite…</button>
            )}
          </div>
          {draft ? (
            <div className="doc essay">
              {report && !busy && (
                <div className="report">
                  <p>{report.text || "Script written."}</p>
                  {report.costUsd > 0 && <small>About ${report.costUsd.toFixed(2)} at API prices, covered by your Claude plan.</small>}
                  <button type="button" className="ghost sm" onClick={() => setReport(null)}>Dismiss</button>
                </div>
              )}
              {!report && draft.report && <p className="claude-says">{draft.report}</p>}
              <h3>{script.title}</h3>
              {draft.sections.map((section, sectionIndex) => (
                <div className="chapter" key={`${sectionIndex}-${section.heading}`}>
                  <h4>{section.heading}</h4>
                  {sectionIndex === 0 && draft.open.length > 0 && (
                    <div className="montage">
                      <span className="label">Montage</span>
                      {draft.open.map((cite, index) => (
                        <ClipChip key={index} cite={cite} sources={sources} />
                      ))}
                    </div>
                  )}
                  {section.paragraphs.map((paragraph, index) => (
                    <ParagraphBlock
                      key={`${sectionIndex}-${index}-${draft.writtenAt}`}
                      paragraph={paragraph}
                      sources={sources}
                      disabled={saving === "saving" || busy}
                      onChange={(next) => editParagraph(sectionIndex, index, next)}
                    />
                  ))}
                </div>
              ))}
              {times && <p className="length total">{commentaryTotal(draft)}</p>}
              {draft.checks.length > 0 && (
                <div className="checks-before">
                  <h4>Checks before recording</h4>
                  <ul>
                    {draft.checks.map((check, index) => (
                      <li key={index}>{check}</li>
                    ))}
                  </ul>
                </div>
              )}
            </div>
          ) : (
            <div className="start">
              <h3>Write the script</h3>
              <p className="muted">
                Claude Opus reads every source: the brief for the angle, the evidence to quote from, and past scripts for
                the voice. It writes the whole script, and Writer checks every quote and link against the sources.
              </p>
            </div>
          )}
        </section>
      </div>
      <AddSourceDialog
        open={adding}
        script={script}
        onClose={() => setAdding(false)}
        onAdded={(next) => {
          onChange(next);
          setAdding(false);
        }}
      />
      <ReadDialog id={script.id} source={reading} onClose={() => setReading(null)} />
      <DocsDialog
        open={docsOpen}
        script={script}
        onClose={() => setDocsOpen(false)}
        onChange={onChange}
        onUseComments={(note) => {
          setNoteSeed(`Apply these comments from the producer on the exported Doc, and keep everything else as it is:\n${note}`);
          setRewriting(true);
        }}
      />
    </div>
  );
}

const commentaryTotal = (draft: Draft) => {
  const { narrationMs, clipsMs, totalMs } = timing(draft);
  return `About ${clock(narrationMs)} of narration + ${clock(clipsMs)} of clips = ${clock(totalMs)}`;
};

function origin(source: Source) {
  if (source.origin === "pasted") return "pasted";
  if (source.origin.startsWith("writer:")) return "from Writer";
  if (source.origin.startsWith("http")) {
    try {
      return new URL(source.origin).host.replace(/^www\./, "");
    } catch {
      return "link";
    }
  }
  return source.origin.split("/").pop() ?? source.origin;
}

function ClipChip({ cite, sources }: { cite: Cite; sources: Source[] }) {
  const source = sources.find((item) => item.id === cite.source);
  const title = [cite.quote ? `“${cite.quote}”` : "No speech", `From ${cite.source}${source ? ` · ${source.title}` : ""}`, cite.link ?? ""]
    .filter(Boolean)
    .join("\n");
  const body = (
    <>
      <span className="mono">{clock(cite.startMs)}-{clock(cite.endMs)}</span>
      <span>{cite.video}</span>
      {cite.verify && <span className="verify" title={cite.verify}>verify</span>}
    </>
  );
  return cite.link ? (
    <a className="clip-chip" href={cite.link} target="_blank" rel="noreferrer" title={title}>
      {body}
    </a>
  ) : (
    <span className="clip-chip" title={title}>
      {body}
    </span>
  );
}

function ParagraphBlock({
  paragraph,
  sources,
  disabled,
  onChange,
}: {
  paragraph: Paragraph;
  sources: Source[];
  disabled: boolean;
  onChange: (next: Paragraph | null) => void;
}) {
  const setPiece = (index: number, text: string) =>
    onChange({ ...paragraph, pieces: paragraph.pieces.map((piece, at) => (at === index && piece.kind === "say" ? { ...piece, text } : piece)) });
  const clips = paragraph.pieces.filter((piece) => piece.kind === "clip") as Cite[];
  return (
    <div className="para">
      {paragraph.pieces.map((piece, index) =>
        piece.kind === "say" ? (
          <Grow key={index} value={piece.text} disabled={disabled} onCommit={(text) => setPiece(index, text)} />
        ) : (
          <div className="clip-line" key={index}>
            <ClipChip cite={piece} sources={sources} />
            {piece.quote && <q>{piece.quote}</q>}
          </div>
        ),
      )}
      {clips.filter((clip) => clip.verify).map((clip, index) => (
        <p className="editor-note verify-note" key={`v${index}`}>
          Verify {cue(clip)}: {clip.verify}
        </p>
      ))}
      {paragraph.note !== undefined && (
        <div className="editor-note">
          <span>For editor</span>
          <Grow
            value={paragraph.note}
            disabled={disabled}
            placeholder="What to show, blur or bleep"
            onCommit={(note) => onChange({ ...paragraph, note: note.trim() ? note : undefined })}
          />
        </div>
      )}
      <div className="para-tools">
        {paragraph.note === undefined && (
          <button type="button" className="ghost sm" disabled={disabled} onClick={() => onChange({ ...paragraph, note: "" })}>
            + Editor note
          </button>
        )}
        <button type="button" className="ghost sm danger-text" disabled={disabled} onClick={() => onChange(null)}>
          Remove paragraph
        </button>
      </div>
    </div>
  );
}

/** A textarea that grows with its text and saves when it loses focus. */
function Grow({
  value,
  disabled,
  placeholder,
  onCommit,
}: {
  value: string;
  disabled: boolean;
  placeholder?: string;
  onCommit: (text: string) => void;
}) {
  const [text, setText] = useState(value);
  const area = useRef<HTMLTextAreaElement>(null);
  useEffect(() => setText(value), [value]);
  useLayoutEffect(() => {
    const node = area.current;
    if (!node) return;
    node.style.height = "auto";
    node.style.height = `${node.scrollHeight}px`;
  }, [text]);
  return (
    <textarea
      ref={area}
      rows={1}
      value={text}
      spellCheck
      disabled={disabled}
      placeholder={placeholder}
      onChange={(event) => setText(event.target.value)}
      onBlur={() => text !== value && onCommit(text)}
    />
  );
}

function Builder({
  ready,
  started,
  stage,
  rewrite,
  seed = "",
  onBuild,
  onCancel,
}: {
  ready: boolean;
  started: number | null;
  stage: string;
  rewrite: boolean;
  /** Notes to start from, such as comments read back from the Doc. */
  seed?: string;
  onBuild: (note: string) => Promise<boolean>;
  onCancel?: () => void;
}) {
  const [note, setNote] = useState(seed);
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (started === null) return;
    setNow(Date.now());
    const tick = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(tick);
  }, [started]);
  if (started !== null) {
    return (
      <div className="builder busy">
        <div className="thinking">
          <span className="dot live" />
          <span>{stage && stage !== "Writing the script" ? `${stage}…` : "Opus is writing the script"}</span>
          <span className="mono faint">{clock(Math.max(0, now - started))}</span>
        </div>
        <span className="faint">A full script from long sources takes 10 to 20 minutes. You can close this window; it keeps going.</span>
      </div>
    );
  }
  return (
    <div className="builder">
      {rewrite && <strong>Rewrite the script</strong>}
      <textarea
        value={note}
        rows={seed ? 8 : 2}
        maxLength={8000}
        placeholder="Optional notes for Claude, e.g. aim for 15 minutes of narration, open on the Christian line"
        onChange={(event) => setNote(event.target.value)}
      />
      <div className="row">
        <span className="faint">
          {!ready ? "Add the brief or some evidence first." : rewrite ? "This replaces the whole script." : "Runs on Opus through your Claude login."}
        </span>
        <span className="spacer" />
        {onCancel && <button type="button" className="ghost sm" onClick={onCancel}>Cancel</button>}
        <button
          type="button"
          className={rewrite ? "sm danger" : "primary"}
          disabled={!ready}
          onClick={() => void onBuild(note).then((done) => done && setNote(""))}
        >
          {rewrite ? "Replace script" : "Write script"}
        </button>
      </div>
    </div>
  );
}

type Kind = "link" | "file" | "text" | "writer";

function AddSourceDialog({
  open,
  script,
  onClose,
  onAdded,
}: {
  open: boolean;
  script: Script;
  onClose: () => void;
  onAdded: (script: Script) => void;
}) {
  const [kind, setKind] = useState<Kind>("link");
  const [value, setValue] = useState("");
  const [title, setTitle] = useState("");
  const [role, setRole] = useState<Role>("evidence");
  const [others, setOthers] = useState<Summary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setBusy(false);
    setValue("");
    setTitle("");
    const hasBrief = script.commentary?.sources.some((source) => source.role === "brief");
    setRole(hasBrief ? "evidence" : "brief");
    void listScripts()
      .then((rows) => setOthers(rows.filter((row) => row.sourceKind !== "commentary" && row.wordCount > 0)))
      .catch(() => setOthers([]));
  }, [open, script.commentary?.sources]);

  const choose = async () => {
    const picked = await pickFile({ multiple: false, filters: [{ name: "Text", extensions: ["txt", "md", "markdown", "srt", "vtt", "csv", "json"] }] });
    if (typeof picked === "string") setValue(picked);
  };

  const submit = async () => {
    const input = value.trim();
    if (!input) return;
    const adding: Adding =
      kind === "link"
        ? { kind: "link", url: input }
        : kind === "file"
          ? { kind: "file", path: input }
          : kind === "text"
            ? { kind: "text", title, text: value }
            : { kind: "writer", id: input };
    setBusy(true);
    setError(null);
    try {
      onAdded(await addSource(script.id, adding, role));
    } catch (cause) {
      setError(message(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Add a source"
      description="Claude reads every source in full when it writes the script."
      width={520}
      footer={
        <>
          {error && <span className="bad dialog-error">{error}</span>}
          <span className="spacer" />
          <button type="button" className="ghost" onClick={onClose}>Cancel</button>
          <button type="submit" form="add-source" className="primary" disabled={busy || !value.trim()}>
            {busy ? "Reading…" : "Add"}
          </button>
        </>
      }
    >
      <form
        id="add-source"
        className="importer"
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
        <Tabs
          value={kind}
          onChange={(next) => {
            setKind(next);
            setValue("");
            setError(null);
          }}
          options={[
            { value: "link", label: <><Icon name="drive" size={14} />Link</> },
            { value: "file", label: <><Icon name="file" size={14} />This Mac</> },
            { value: "text", label: "Paste" },
            { value: "writer", label: <><Icon name="stack" size={14} />A transcript</> },
          ]}
        />
        <div className="source" key={kind}>
          {kind === "link" && (
            <>
              <input
                data-autofocus
                value={value}
                placeholder="https://docs.google.com/document/d/…"
                spellCheck={false}
                onChange={(event) => setValue(event.target.value)}
              />
              <p className="hint">A Google Doc, a Drive text file, or a web page. Shared with "Anyone with the link" or with Repack's Drive account.</p>
            </>
          )}
          {kind === "file" &&
            (inTauri() ? (
              <button type="button" className="drop" onClick={() => void choose()}>
                {value ? (
                  <>
                    <strong>{value.split("/").pop()}</strong>
                    <span>{value}</span>
                  </>
                ) : (
                  <>
                    <strong>Choose a text file…</strong>
                    <span>TXT, MD, SRT, VTT, CSV or JSON</span>
                  </>
                )}
              </button>
            ) : (
              <input data-autofocus value={value} placeholder="/Users/you/Downloads/report.txt" spellCheck={false} onChange={(event) => setValue(event.target.value)} />
            ))}
          {kind === "text" && (
            <>
              <input data-autofocus value={title} maxLength={180} placeholder="Title, e.g. Reddit capture u/mrbeast6000" onChange={(event) => setTitle(event.target.value)} />
              <textarea className="links" rows={8} value={value} placeholder="Paste the text" onChange={(event) => setValue(event.target.value)} />
            </>
          )}
          {kind === "writer" &&
            (others.length ? (
              <div className="checklist">
                {others.map((row) => (
                  <label key={row.id}>
                    <input type="radio" name="writer-source" checked={value === row.id} onChange={() => setValue(row.id)} />
                    <span>{row.title}</span>
                    <span className="faint">{clock(row.durationMs)}</span>
                  </label>
                ))}
              </div>
            ) : (
              <p className="hint">No transcribed footage in Writer yet. Import a video as a new script first, then add its transcript here.</p>
            ))}
          <div className="row role-pick">
            <span className="faint">Use it as</span>
            <Tabs value={role} onChange={setRole} options={ROLES.map((item) => ({ value: item.value, label: item.label }))} />
          </div>
          <p className="hint">{ROLES.find((item) => item.value === role)?.hint}.</p>
        </div>
      </form>
    </Dialog>
  );
}

function ReadDialog({ id, source, onClose }: { id: string; source: Source | null; onClose: () => void }) {
  const [text, setText] = useState<string | null>(null);
  const shown = useRef(source);
  if (source) shown.current = source;
  useEffect(() => {
    if (!source) return;
    setText(null);
    void sourceText(id, source.id)
      .then(setText)
      .catch((cause) => setText(message(cause)));
  }, [id, source]);
  const limit = 200_000;
  return (
    <Dialog open={!!source} onClose={onClose} title={`${shown.current?.id ?? ""} · ${shown.current?.title ?? ""}`} width={760}
      footer={<><span className="spacer" /><button type="button" className="ghost" data-autofocus onClick={onClose}>Close</button></>}
    >
      <pre className="source-text">
        {text === null ? "Reading…" : text.length > limit ? `${text.slice(0, limit)}\n\n… ${size(text.length - limit)} more characters` : text}
      </pre>
    </Dialog>
  );
}
