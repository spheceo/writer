import { type ReactNode, useEffect, useState } from "react";
import { forgetLesson, getLessons, keepLesson, message, pullFeedback, setFeedbackItem, setLesson } from "./api";
import type { Feedback, FeedbackItem, Grade, LearnStep, Lesson, LessonRound, ReviewNote, ScriptChange } from "./types";
import { Icon } from "./ui";

const GRADES: Record<Grade, string> = { fixed: "Fixed", partly: "Partly", missed: "Missed", wrong: "Wrong" };

const percent = (score: number) => `${Math.round(score * 100)}%`;

/** One paragraph before and after. */
function ChangeView({ label, change }: { label?: string; change: ScriptChange }) {
  return (
    <div className="fb-change">
      <span className="faint">
        {label ? `${label} · ` : ""}
        {change.place}
      </span>
      {change.before && <p className="fb-before">{change.before}</p>}
      {change.after ? <p className="fb-after">{change.after}</p> : <p className="faint">Removed.</p>}
    </div>
  );
}

function ItemRow({ item, busy, onChange }: { item: FeedbackItem; busy: boolean; onChange: (change: { skip?: boolean; note?: string }) => void }) {
  const [note, setNote] = useState(item.note);
  useEffect(() => setNote(item.note), [item.note]);
  const bare = item.kind === "highlight" && !item.comment;
  return (
    <li className="fb-item" data-skip={item.skip || undefined}>
      <div className="fb-item-head">
        <label className="fb-use" title={item.skip ? "Left out of the key, the lesson and the grading" : "Used"}>
          <input type="checkbox" checked={!item.skip} disabled={busy} onChange={(event) => onChange({ skip: !event.target.checked })} />
          <span className="badge mono">{item.id}</span>
        </label>
        <span className="chip">
          {item.kind === "highlight" && <span className="fb-swatch" style={{ background: item.color }} />}
          {item.kind === "comment" ? "Comment" : "Highlight"}
        </span>
        <span className="faint fb-place">
          {item.place ?? "Not found in the script"}
          {item.version ? ` · v${item.version}` : ""}
          {item.status === "done" ? ` · done${item.doneIn ? ` in v${item.doneIn}` : ""}` : ""}
        </span>
      </div>
      {item.quote && <blockquote>{item.quote}</blockquote>}
      {item.replacement !== undefined && <p className="fb-after">{item.replacement ? `Suggests: ${item.replacement}` : "Suggests deleting these words."}</p>}
      {item.comment && (
        <p>
          {item.author && <strong>{item.author}: </strong>}
          {item.comment}
        </p>
      )}
      {item.replies.map((reply, index) => (
        <p key={index} className="fb-reply">{reply}</p>
      ))}
      <input
        className="fb-note"
        value={note}
        disabled={busy}
        placeholder={bare ? "What's wrong here? (optional)" : "Add a note (optional)"}
        onChange={(event) => setNote(event.target.value)}
        onBlur={() => note.trim() !== item.note && onChange({ note })}
      />
    </li>
  );
}

function RoundView({ round, key_, items, open, busy, onToggle, onFinish }: {
  round: LessonRound;
  key_: ScriptChange[];
  items: FeedbackItem[];
  open: boolean;
  busy: boolean;
  onToggle: () => void;
  onFinish: () => void;
}) {
  const [shown, setShown] = useState<string | null>(null);
  const count = (grade: Grade) => round.verdicts.filter((verdict) => verdict.grade === grade).length;
  const hurt = round.extras.filter((extra) => extra.harmful).length;
  const change = (id: string) => round.changes[Number(id.replace(/\D/g, "")) - 1];
  return (
    <section className="fb-round" data-open={open || undefined}>
      <button type="button" className="fb-round-head" onClick={onToggle} aria-expanded={open}>
        <strong>Round {round.number}</strong>
        <span className="fb-score" style={{ ["--score" as string]: round.score }}>{percent(round.score)}</span>
        <span className="faint">
          {count("fixed")} fixed{count("partly") ? `, ${count("partly")} partly` : ""}
          {count("missed") ? `, ${count("missed")} missed` : ""}
          {count("wrong") ? `, ${count("wrong")} wrong` : ""}
          {hurt ? ` · ${hurt} hurt` : ""}
        </span>
        <Icon name="chevron" size={12} />
      </button>
      {open && (
        <div className="fb-round-body">
          {round.summary && <p>{round.summary}</p>}
          <ul className="fb-verdicts">
            {round.verdicts.map((verdict) => {
              const item = items.find((found) => found.id === verdict.item);
              const keyed = key_.filter((found) => item?.place && found.place === item.place);
              return (
                <li key={verdict.item}>
                  <div className="fb-verdict-head">
                    <span className={`chip grade-${verdict.grade}`}>{GRADES[verdict.grade]}</span>
                    <span className="badge mono">{verdict.item}</span>
                    <span className="fb-why">{verdict.why}</span>
                  </div>
                  {(verdict.changes.length > 0 || keyed.length > 0) && (
                    <button type="button" className="link-button" onClick={() => setShown(shown === verdict.item ? null : verdict.item)}>
                      {shown === verdict.item ? "Hide the changes" : "Show the changes"}
                    </button>
                  )}
                  {shown === verdict.item && (
                    <>
                      {verdict.changes.map((id) => change(id) && <ChangeView key={id} label={`Round ${id}`} change={change(id)} />)}
                      {keyed.map((found, index) => (
                        <ChangeView key={`k${index}`} label="Answer key" change={found} />
                      ))}
                    </>
                  )}
                </li>
              );
            })}
          </ul>
          {round.extras.length > 0 && (
            <>
              <p><strong>Changes nobody asked for</strong></p>
              <ul className="fb-verdicts">
                {round.extras.map((extra) => (
                  <li key={extra.change}>
                    <div className="fb-verdict-head">
                      <span className={`chip ${extra.harmful ? "grade-wrong" : ""}`}>{extra.harmful ? "Hurt" : "Fine"}</span>
                      <span className="badge mono">{extra.change}</span>
                      <span className="fb-why">{extra.why}</span>
                    </div>
                    {change(extra.change) && <ChangeView change={change(extra.change)} />}
                  </li>
                ))}
              </ul>
            </>
          )}
          <details className="fb-lesson-then">
            <summary>The lesson this round read</summary>
            <pre>{round.lesson}</pre>
          </details>
          <button
            type="button"
            className="secondary sm"
            disabled={busy}
            title="Make this round's script the script, with the items it didn't fix applied directly"
            onClick={onFinish}
          >
            Use this round's script
          </button>
        </div>
      )}
    </section>
  );
}

/**
 * Learning from a review: the Doc's comments and highlights, the lesson
 * Claude draws from them, and rounds that test whether a fresh Claude, given
 * only the lesson, finds and fixes the same problems.
 */
export function FeedbackPanel({
  status,
  id,
  feedback,
  notes,
  shared,
  busy,
  canPull,
  onFeedback,
  onStep,
  onError,
}: {
  /** Claude at work on a step, or what the last step said. */
  status: ReactNode;
  id: string;
  feedback: Feedback | null;
  /** The notes on the script's review page, reviewers' and Writer's. */
  notes: ReviewNote[];
  /** Shared for review: notes can be left and read. */
  shared: boolean;
  busy: boolean;
  canPull: boolean;
  onFeedback: (feedback: Feedback) => void;
  onStep: (step: LearnStep) => void;
  onError: (error: string) => void;
}) {
  const [pulling, setPulling] = useState(false);
  const top = notes.filter((note) => !note.parentId);
  const [lesson, setLessonText] = useState(feedback?.lesson ?? "");
  const [leaks, setLeaks] = useState<string[]>([]);
  const [open, setOpen] = useState<number | null>(null);
  const [itemsOpen, setItemsOpen] = useState(true);
  const [kept, setKept] = useState<Lesson[]>([]);
  useEffect(() => setLessonText(feedback?.lesson ?? ""), [feedback?.lesson]);
  useEffect(() => {
    void getLessons().then(setKept, () => {});
  }, []);
  const rounds = feedback?.rounds ?? [];
  const last = rounds[rounds.length - 1];
  useEffect(() => setOpen(last?.number ?? null), [last?.number]);
  useEffect(() => setItemsOpen(rounds.length === 0), [feedback?.pulledAt, rounds.length]);

  const pull = async () => {
    if (rounds.length > 0 && !window.confirm("Read the review's notes again? This starts over: the answer key, the lesson and the rounds go.")) return;
    setPulling(true);
    try {
      onFeedback(await pullFeedback(id));
      setLeaks([]);
    } catch (cause) {
      onError(message(cause));
    } finally {
      setPulling(false);
    }
  };
  const item = async (itemId: string, change: { skip?: boolean; note?: string }) => {
    try {
      onFeedback(await setFeedbackItem(id, itemId, change));
    } catch (cause) {
      onError(message(cause));
    }
  };
  const saveLesson = async () => {
    try {
      const saved = await setLesson(id, lesson);
      onFeedback(saved.feedback);
      setLeaks(saved.leaks);
    } catch (cause) {
      onError(message(cause));
    }
  };
  const keep = async () => {
    try {
      setKept(await keepLesson(id));
    } catch (cause) {
      onError(message(cause));
    }
  };
  const forget = async (lessonId: string) => {
    try {
      setKept(await forgetLesson(lessonId));
    } catch (cause) {
      onError(message(cause));
    }
  };

  const used = feedback?.items.filter((found) => !found.skip).length ?? 0;
  const edited = !!feedback && lesson.trim() !== feedback.lesson.trim();
  const keptThis = !!feedback && kept.some((found) => found.script === id && found.pulledAt === feedback.pulledAt);
  // Notes left since the feedback was read: reading again brings them in.
  const newer = feedback ? top.filter((note) => (note.createdAt ?? 0) > feedback.pulledAt).length : 0;

  return (
    <div className="sources found-clips feedback-panel">
      {status && <div className="fb-status">{status}</div>}
      {!feedback ? (
        <div className="empty">
          <h3 className="fb-teach-title">Teach Writer from these comments</h3>
          <p>
            Opus makes an answer key by applying the comments above, then writes a lesson from them in general terms. In each round a fresh
            Opus gets only the script and the lesson, finds the problems itself and fixes them, and is graded against the comments.
          </p>
          <p className="hint">Run rounds until the match is high, then keep the lesson so every later script reads it. Each round takes about 15 minutes.</p>
          {!canPull ? (
            <p className="hint">Write the script first.</p>
          ) : !shared ? (
            <p className="hint">Share the script for review (Share, at the top): its notes are the feedback.</p>
          ) : (
            <>
              <button type="button" className="sm" disabled={pulling || busy || top.length === 0} onClick={() => void pull()}>
                {pulling ? "Reading…" : top.length ? `Teach from ${top.length} ${top.length === 1 ? "comment" : "comments"}` : "No comments yet"}
              </button>
            </>
          )}
        </div>
      ) : (
        <>
          <div className="found-head">
            <p className="faint">
              <a href={feedback.docUrl} target="_blank" rel="noreferrer">{feedback.docUrl.includes("docs.google.com") ? "The Doc" : "The review"}</a> · {used} of {feedback.items.length} notes used
            </p>
            <button type="button" className="tool" disabled={busy || pulling || !shared} title="Starts over from the review's notes as they are now" onClick={() => void pull()}>
              {pulling ? "Reading…" : newer ? `Read again (${newer} new)` : "Read again"}
            </button>
          </div>

          {feedback.problems.length > 0 && (
            <ul className="fb-problems faint">
              {feedback.problems.map((problem, index) => (
                <li key={index}>{problem}</li>
              ))}
            </ul>
          )}

          <section className="fb-section">
            <button type="button" className="fb-section-head" onClick={() => setItemsOpen(!itemsOpen)} aria-expanded={itemsOpen}>
              <h3>Feedback</h3>
              <span className="faint">{feedback.items.length}</span>
              <Icon name="chevron" size={12} />
            </button>
            {itemsOpen && (
              <ul className="fb-items">
                {feedback.items.map((found) => (
                  <ItemRow key={found.id} item={found} busy={busy} onChange={(change) => void item(found.id, change)} />
                ))}
              </ul>
            )}
          </section>

          <section className="fb-section">
            <div className="fb-section-head">
              <h3>Lesson{feedback.title ? `: ${feedback.title.replace(/^Lesson:\s*/i, "")}` : ""}</h3>
            </div>
            <textarea
              className="fb-lesson"
              value={lesson}
              disabled={busy}
              rows={lesson ? 12 : 3}
              placeholder="Round 1 writes it from the feedback. Or write your own: the kinds of problem, how to spot them, how to fix them. Never point at this script."
              onChange={(event) => setLessonText(event.target.value)}
            />
            {edited && (
              <div className="fb-actions">
                <button type="button" className="sm" disabled={busy} onClick={() => void saveLesson()}>Save the lesson</button>
                <button type="button" className="ghost sm" onClick={() => setLessonText(feedback.lesson)}>Undo</button>
              </div>
            )}
            {leaks.length > 0 && <p className="fb-leaks">It still points at the script: {leaks.join(", ")}. The round would see the answers.</p>}
          </section>

          <div className="fb-actions">
            <button type="button" className="sm" disabled={busy || edited || used === 0} title={edited ? "Save the lesson first" : "A fresh Opus fixes the script from the lesson alone, then it's graded"} onClick={() => onStep({ step: "round" })}>
              Run round {(last?.number ?? 0) + 1}
            </button>
            <button type="button" className="secondary sm" disabled={busy || edited || !last} title="Opus rewrites the lesson from what the last round missed" onClick={() => onStep({ step: "improve" })}>
              Improve the lesson
            </button>
            <button type="button" className="secondary sm" disabled={busy || edited || !feedback.lesson} title="Every later commentary build reads it" onClick={() => void keep()}>
              {keptThis ? "Keep again" : "Keep the lesson"}
            </button>
          </div>

          {feedback.key && (
            <details className="fb-key">
              <summary>Answer key · {feedback.key.length} changes</summary>
              {feedback.key.map((change, index) => (
                <ChangeView key={index} label={`K${index + 1}`} change={change} />
              ))}
            </details>
          )}
          {[...rounds].reverse().map((round) => (
            <RoundView
              key={round.number}
              round={round}
              key_={feedback.key ?? []}
              items={feedback.items}
              open={open === round.number}
              busy={busy}
              onToggle={() => setOpen(open === round.number ? null : round.number)}
              onFinish={() => {
                if (window.confirm(`Make round ${round.number}'s script the script? What it didn't fix is applied directly. The current script is replaced.`)) {
                  onStep({ step: "finish", round: round.number });
                }
              }}
            />
          ))}
        </>
      )}

      {kept.length > 0 && (
        <section className="fb-section fb-kept">
          <div className="fb-section-head">
            <h3>Lessons every build reads</h3>
            <span className="faint">{kept.length}</span>
          </div>
          {kept.map((found) => (
            <details key={found.id}>
              <summary>
                <span>{found.title.replace(/^Lesson:\s*/i, "")}</span>
                <span className="faint">
                  {found.scriptTitle}
                  {found.score !== undefined ? ` · ${percent(found.score)}` : ""}
                </span>
              </summary>
              <pre>{found.text}</pre>
              <button type="button" className="ghost sm" onClick={() => void forget(found.id)}>Forget it</button>
            </details>
          ))}
        </section>
      )}
    </div>
  );
}
