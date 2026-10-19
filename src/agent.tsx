import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { clock } from "./clips";
import { Icon } from "./ui";

/** Time since a moment, ticking each second. */
export function Elapsed({ since }: { since: number | null }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (since === null) return;
    const tick = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(tick);
  }, [since]);
  return <>{clock(Math.max(0, now - (since ?? now)))}</>;
}

/** The conversation, following itself down as it grows. */
export function ChatLog({ trigger, children }: { trigger: string; children: ReactNode }) {
  const log = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const node = log.current;
    if (node) node.scrollTop = node.scrollHeight;
  }, [trigger]);
  return (
    <div className="chat-log" ref={log}>
      {children}
    </div>
  );
}

/** Claude at work: what it is on, and for how long. */
export function Working({ stage, since, hint }: { stage: string; since: number | null; hint: string }) {
  return (
    <div className="turn working">
      <div className="thinking">
        <span className="dot live" />
        <span>{stage === "Editing the script" ? "Sonnet is editing the script" : stage && stage !== "Writing the script" ? `${stage}…` : "Opus is writing the script"}</span>
        <span className="mono faint">
          <Elapsed since={since} />
        </span>
      </div>
      <small className="faint">{hint}</small>
    </div>
  );
}

/** What the producer asked, as their side of the conversation. */
export function Asked({ text }: { text: string }) {
  return (
    <div className="user">
      <div className="bubble">{text}</div>
    </div>
  );
}

/** What Claude said, a paragraph at a time. */
export function Said({ text, costUsd, failed }: { text: string; costUsd?: number; failed?: boolean }) {
  return (
    <div className={failed ? "turn failed" : "turn"}>
      {text.split(/\n{2,}/).map((part, index) => (
        <p key={index}>{part}</p>
      ))}
      {!!costUsd && costUsd > 0 && <small className="faint">About ${costUsd.toFixed(2)} at API prices, covered by your Claude plan.</small>}
    </div>
  );
}

/**
 * Repack's composer: a growing field, the model it runs on, and a send
 * button. Enter sends; Shift-Enter starts a new line.
 */
export function Composer({
  value,
  disabled,
  canSend,
  placeholder,
  hint,
  extra,
  label,
  model = "Opus 5.5",
  onChange,
  onSend,
}: {
  value: string;
  disabled: boolean;
  canSend: boolean;
  placeholder: string;
  hint?: string;
  /** More controls beside the model, such as the format. */
  extra?: ReactNode;
  label: string;
  /** The model the message goes to. */
  model?: string;
  onChange: (value: string) => void;
  onSend: () => void;
}) {
  const area = useRef<HTMLTextAreaElement>(null);
  useLayoutEffect(() => {
    const node = area.current;
    if (!node) return;
    node.style.height = "auto";
    node.style.height = `${Math.min(200, Math.max(44, node.scrollHeight))}px`;
  }, [value]);
  return (
    <div className="composer">
      <textarea
        ref={area}
        rows={2}
        value={value}
        disabled={disabled}
        maxLength={8000}
        placeholder={placeholder}
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
            event.preventDefault();
            if (canSend) onSend();
          }
        }}
      />
      <div className="composer-footer">
        <span className="model" title={`${model} through your Claude login`}>
          <svg className="claude-spark" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true">
            <path d="M6 .5v11M.5 6h11M2.1 2.1l7.8 7.8M9.9 2.1l-7.8 7.8" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" />
          </svg>
          {model}
        </span>
        {extra}
        <span className="spacer" />
        {hint && <span className="faint composer-hint">{hint}</span>}
        <button type="button" className="send" aria-label={label} title={label} disabled={!canSend} onClick={onSend}>
          <Icon name="send" size={15} />
        </button>
      </div>
    </div>
  );
}
