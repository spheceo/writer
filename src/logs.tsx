import { useEffect, useMemo, useRef, useState } from "react";
import { getLogs, type LogLine, message, revealLogs } from "./api";
import { Dialog, Icon, Select } from "./ui";

/**
 * Writer's log, as it's written (log.rs): a live tail of the last 2000
 * lines, searchable, by subsystem, all or only problems, to copy or find in
 * Finder.
 */
export function LogsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [lines, setLines] = useState<LogLine[]>([]);
  const [file, setFile] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [area, setArea] = useState("all");
  const [problems, setProblems] = useState(false);
  const [copied, setCopied] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const last = useRef(0);
  const list = useRef<HTMLDivElement>(null);
  const stuck = useRef(true);

  useEffect(() => {
    if (!open) return;
    let live = true;
    last.current = 0;
    setLines([]);
    const ask = () =>
      void getLogs(last.current).then(
        (found) => {
          if (!live) return;
          setFile(found.file);
          if (!found.lines.length) return;
          last.current = found.lines[found.lines.length - 1].at;
          setLines((known) => [...known, ...found.lines].slice(-2000));
        },
        () => {},
      );
    ask();
    const timer = window.setInterval(ask, 1500);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [open]);

  const areas = useMemo(() => [...new Set(lines.map((line) => line.area))].sort(), [lines]);
  const shown = useMemo(() => {
    const words = query.trim().toLowerCase();
    return lines.filter((line) => (area === "all" || line.area === area) && (!problems || line.level !== "INFO") && (!words || `${line.area} ${line.message}`.toLowerCase().includes(words)));
  }, [lines, query, area, problems]);

  // Follows the newest line unless the reader has scrolled up.
  useEffect(() => {
    const node = list.current;
    if (node && stuck.current) node.scrollTop = node.scrollHeight;
  }, [shown]);

  const text = (items: LogLine[]) => items.map((line) => `${line.time} ${line.level.padEnd(5)} [${line.area}] ${line.message}`).join("\n");
  const copy = async () => {
    await navigator.clipboard.writeText(text(shown));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Logs"
      description={file ? `Writer's log, as it's written: ${file}` : "Writer's log, as it's written."}
      width={980}
      footer={
        <>
          {problem && <span className="bad">{problem}</span>}
          <span className="faint">{shown.length === lines.length ? `${lines.length} lines` : `${shown.length} of ${lines.length} lines`}</span>
          <span className="spacer" />
          <button type="button" className="ghost" onClick={() => void revealLogs().catch((cause) => setProblem(message(cause)))}>
            Show in Finder
          </button>
          <button type="button" className="ghost" disabled={!shown.length} onClick={() => void copy()}>
            <Icon name={copied ? "check" : "copy"} size={14} />
            {copied ? "Copied" : "Copy"}
          </button>
          <button type="button" className="primary" onClick={onClose}>Done</button>
        </>
      }
    >
      <div className="logs-tools">
        <input type="search" placeholder="Search the log" value={query} onChange={(event) => setQuery(event.target.value)} aria-label="Search the log" />
        <Select value={area} label="Subsystem" onChange={setArea} options={[{ value: "all", label: "Every subsystem" }, ...areas.map((item) => ({ value: item, label: item }))]} />
        <div className="segmented" role="group" aria-label="Which lines">
          <button type="button" className={problems ? "" : "on"} onClick={() => setProblems(false)}>All</button>
          <button type="button" className={problems ? "on" : ""} onClick={() => setProblems(true)}>Problems</button>
        </div>
      </div>
      <div
        className="logs-lines"
        ref={list}
        onScroll={(event) => {
          const node = event.currentTarget;
          stuck.current = node.scrollHeight - node.scrollTop - node.clientHeight < 24;
        }}
      >
        {shown.length === 0 && <p className="faint">{lines.length ? "No line matches." : "Nothing logged yet."}</p>}
        {shown.map((line, index) => (
          <div key={`${line.at}-${index}`} className="log-line" data-level={line.level}>
            <span className="log-time">{line.time.slice(11)}</span>
            <span className="log-level">{line.level}</span>
            <span className="log-area">{line.area}</span>
            <span className="log-message">{line.message}</span>
          </div>
        ))}
      </div>
    </Dialog>
  );
}
