import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";

/** Keeps something mounted for the length of its closing animation. */
function usePresence(open: boolean, ms = 140) {
  const [mounted, setMounted] = useState(open);
  const [closing, setClosing] = useState(false);
  useEffect(() => {
    if (open) {
      setMounted(true);
      setClosing(false);
      return;
    }
    if (!mounted) return;
    setClosing(true);
    const timer = window.setTimeout(() => {
      setMounted(false);
      setClosing(false);
    }, ms);
    return () => window.clearTimeout(timer);
  }, [open, mounted, ms]);
  return { mounted, closing };
}

const FOCUSABLE = 'button:not(:disabled), input:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

export function Dialog({
  open,
  onClose,
  title,
  description,
  children,
  footer,
  width = 440,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  width?: number;
}) {
  const { mounted, closing } = usePresence(open);
  const box = useRef<HTMLDivElement>(null);
  const close = useRef(onClose);
  close.current = onClose;

  useEffect(() => {
    if (!open) return;
    const before = document.activeElement as HTMLElement | null;
    const node = box.current;
    const first = node?.querySelector<HTMLElement>("[data-autofocus]") ?? node?.querySelector<HTMLElement>("input, textarea");
    (first ?? node)?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        close.current();
      }
      if (event.key !== "Tab" || !box.current) return;
      const items = [...box.current.querySelectorAll<HTMLElement>(FOCUSABLE)];
      if (!items.length) return;
      const [head, tail] = [items[0], items[items.length - 1]];
      if (event.shiftKey && document.activeElement === head) {
        event.preventDefault();
        tail.focus();
      } else if (!event.shiftKey && document.activeElement === tail) {
        event.preventDefault();
        head.focus();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      before?.focus?.();
    };
  }, [open]);

  if (!mounted) return null;
  return (
    <div className={closing ? "overlay out" : "overlay"} onPointerDown={(event) => event.target === event.currentTarget && onClose()}>
      <div
        ref={box}
        className={closing ? "dialog out" : "dialog"}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        style={{ width }}
      >
        <div className="dialog-head">
          <h2>{title}</h2>
          {description && <p>{description}</p>}
        </div>
        {children && <div className="dialog-body">{children}</div>}
        {footer && <div className="dialog-foot">{footer}</div>}
      </div>
    </div>
  );
}

export type MenuItem = { label: string; onSelect: () => void; danger?: boolean; disabled?: boolean; hint?: string };

export function Menu({ items, label = "More" }: { items: MenuItem[]; label?: string }) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const { mounted, closing } = usePresence(open, 110);
  const root = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLDivElement>(null);

  // The list mounts a render after opening; take focus once it is there.
  useEffect(() => {
    if (open && mounted) list.current?.focus();
  }, [open, mounted]);

  useEffect(() => {
    if (!open) return;
    setActive(items.findIndex((item) => !item.disabled));
    const onDown = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("pointerdown", onDown);
    return () => window.removeEventListener("pointerdown", onDown);
    // Items are rebuilt on every render; only opening resets the highlight.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const choose = (item: MenuItem | undefined) => {
    if (!item || item.disabled) return;
    setOpen(false);
    item.onSelect();
  };

  const step = (by: number) => {
    if (!items.length) return;
    let next = active;
    for (let tries = 0; tries < items.length; tries++) {
      next = (next + by + items.length) % items.length;
      if (!items[next].disabled) break;
    }
    setActive(next);
  };

  return (
    <div className="menu" ref={root} onClick={(event) => event.stopPropagation()}>
      <button
        type="button"
        className={open ? "icon-button on" : "icon-button"}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <Icon name="more" />
      </button>
      {mounted && (
        <div
          ref={list}
          className={closing ? "menu-list out" : "menu-list"}
          role="menu"
          tabIndex={-1}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") step(1);
            else if (event.key === "ArrowUp") step(-1);
            else if (event.key === "Enter" || event.key === " ") choose(items[active]);
            else if (event.key === "Escape" || event.key === "Tab") setOpen(false);
            else return;
            event.preventDefault();
            event.stopPropagation();
          }}
        >
          {items.map((item, index) => (
            <button
              type="button"
              role="menuitem"
              key={item.label}
              tabIndex={-1}
              disabled={item.disabled}
              className={[index === active ? "active" : "", item.danger ? "danger" : ""].join(" ")}
              onPointerEnter={() => !item.disabled && setActive(index)}
              onClick={() => choose(item)}
            >
              {item.label}
              {item.hint && <span>{item.hint}</span>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** A segmented switch whose highlight slides to the chosen option. */
export function Tabs<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: ReactNode }[];
  onChange: (value: T) => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  const [pill, setPill] = useState<{ left: number; width: number } | null>(null);
  useLayoutEffect(() => {
    const node = root.current?.querySelector<HTMLElement>(`[data-value="${value}"]`);
    if (node) setPill({ left: node.offsetLeft, width: node.offsetWidth });
  }, [value]);
  return (
    <div className="tabs" role="tablist" ref={root}>
      {pill && <span className="tabs-pill" style={{ transform: `translateX(${pill.left - 3}px)`, width: pill.width }} />}
      {options.map((option) => (
        <button
          type="button"
          role="tab"
          key={option.value}
          data-value={option.value}
          aria-selected={option.value === value}
          className={option.value === value ? "on" : ""}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

export type ToastMessage = { id: number; text: ReactNode; action?: { label: string; onClick: () => void } };

export function Toasts({ toasts, onDismiss }: { toasts: ToastMessage[]; onDismiss: (id: number) => void }) {
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((toast) => (
        <Toast key={toast.id} toast={toast} onDismiss={() => onDismiss(toast.id)} />
      ))}
    </div>
  );
}

function Toast({ toast, onDismiss }: { toast: ToastMessage; onDismiss: () => void }) {
  useEffect(() => {
    const timer = window.setTimeout(onDismiss, 6000);
    return () => window.clearTimeout(timer);
  }, [onDismiss]);
  return (
    <div className="toast">
      <span>{toast.text}</span>
      {toast.action && (
        <button
          type="button"
          className="ghost sm"
          onClick={() => {
            toast.action?.onClick();
            onDismiss();
          }}
        >
          {toast.action.label}
        </button>
      )}
    </div>
  );
}

const PATHS: Record<string, ReactNode> = {
  plus: <path d="M8 3.5v9M3.5 8h9" />,
  undo: <path d="M5.5 4 2.75 6.75 5.5 9.5M3 6.75h6.25a3.75 3.75 0 0 1 0 7.5H7" />,
  redo: <path d="m10.5 4 2.75 2.75-2.75 2.75M13 6.75H6.75a3.75 3.75 0 0 0 0 7.5H9" />,
  more: (
    <>
      <circle cx="3.5" cy="8" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="12.5" cy="8" r="1.1" fill="currentColor" stroke="none" />
    </>
  ),
  settings: (
    <>
      <path d="M2.5 4.5h7M12.5 4.5h1M2.5 11.5h1M6.5 11.5h7" />
      <circle cx="11" cy="4.5" r="1.5" />
      <circle cx="5" cy="11.5" r="1.5" />
    </>
  ),
  youtube: (
    <>
      <rect x="1.75" y="3.5" width="12.5" height="9" rx="2.5" />
      <path d="M6.75 6v4l3.25-2z" fill="currentColor" stroke="none" />
    </>
  ),
  drive: <path d="M5.75 2.5h4.5l4 7-2.25 4h-8L1.75 9.5zM5.75 2.5l4 7h4.5M1.75 9.5h8l-2.25 4" />,
  stack: (
    <>
      <rect x="2.25" y="5.25" width="11.5" height="8.5" rx="1.75" />
      <path d="M4 3.25h8M5.75 1.5h4.5" />
    </>
  ),
  file: (
    <>
      <path d="M4 1.75h5l3 3v9.5H4z" />
      <path d="M9 1.75v3h3" />
    </>
  ),
  script: (
    <>
      <path d="M4 1.75h5l3 3v9.5H4z" />
      <path d="M9 1.75v3h3M6 8.25h4M6 10.75h4" />
    </>
  ),
  back: <path d="M13 8H3.5M7.5 4 3.5 8l4 4" />,
  history: <path d="M2.75 8a5.25 5.25 0 1 0 1.6-3.78M2.75 2.75v2.5h2.5M8 5.25V8l2 1.5" />,
  minus: <path d="M3.5 8h9" />,
  chevron: <path d="m4.5 6.25 3.5 3.5 3.5-3.5" />,
  send: <path d="M8 13V3.5M4 7.25 8 3.25l4 4" />,
  copy: (
    <>
      <rect x="5.25" y="5.25" width="8.5" height="8.5" rx="1.75" />
      <path d="M10.75 5.25V3.75a1.5 1.5 0 0 0-1.5-1.5h-5.5a1.5 1.5 0 0 0-1.5 1.5v5.5a1.5 1.5 0 0 0 1.5 1.5h1.5" />
    </>
  ),
  check: <path d="m3.25 8.5 3 3 6.5-7" />,
  doc: (
    <>
      <path d="M4 1.75h5l3 3v9.5H4z" />
      <path d="M9 1.75v3h3M6 8.25h4M6 10.75h2.5" />
    </>
  ),
  note: <path d="M2.75 3.75a1 1 0 0 1 1-1h8.5a1 1 0 0 1 1 1v6.5a1 1 0 0 1-1 1H7L4 13.75v-2.5h-.25a1 1 0 0 1-1-1z" />,
  scissors: (
    <>
      <circle cx="4.25" cy="4.25" r="1.75" />
      <circle cx="4.25" cy="11.75" r="1.75" />
      <path d="M5.75 5.25 13 11.5M5.75 10.75 13 4.5" />
    </>
  ),
  type: <path d="M3.5 4V3h9v1M8 3v10M6 13h4" />,
  trash: <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5" />,
  play: <path d="M5 3.25v9.5L12.75 8z" fill="currentColor" />,
  pause: (
    <>
      <rect x="4" y="3.25" width="2.75" height="9.5" rx="0.5" fill="currentColor" stroke="none" />
      <rect x="9.25" y="3.25" width="2.75" height="9.5" rx="0.5" fill="currentColor" stroke="none" />
    </>
  ),
  start: <path d="M3.5 3v10M13 8H6M9 4.75 5.75 8 9 11.25" />,
  end: <path d="M12.5 3v10M3 8h7M7 4.75 10.25 8 7 11.25" />,
  volume: <path d="M2.75 6.25h2.5L8.5 3.5v9l-3.25-2.75h-2.5zM10.75 5.75a3 3 0 0 1 0 4.5M12.5 4a5.5 5.5 0 0 1 0 8" />,
  mute: <path d="M2.75 6.25h2.5L8.5 3.5v9l-3.25-2.75h-2.5zM10.75 6.25l3 3.5M13.75 6.25l-3 3.5" />,
  expand: <path d="M9.5 2.75h3.75V6.5M6.5 13.25H2.75V9.5M13.25 2.75 9 7M2.75 13.25 7 9" />,
  collapse: <path d="M13 3 9.25 6.75M9.25 3.5v3.25h3.25M3 13l3.75-3.75M6.75 12.5V9.25H3.5" />,
  cloud: <path d="M4.75 12.25h6.5a2.75 2.75 0 0 0 .4-5.47A4 4 0 0 0 4 6.6a2.85 2.85 0 0 0 .75 5.65Z" />,
  keyboard: (
    <>
      <rect x="1.75" y="4" width="12.5" height="8.5" rx="1.5" />
      <path d="M4.5 7h.01M6.8 7h.01M9.2 7h.01M11.5 7h.01M5 9.75h6" />
    </>
  ),
  search: (
    <>
      <circle cx="7" cy="7" r="4.25" />
      <path d="m10.25 10.25 3 3" />
    </>
  ),
  panel: (
    <>
      <rect x="2" y="2.75" width="12" height="10.5" rx="1.75" />
      <path d="M10 2.75v10.5" />
    </>
  ),
};

export function Icon({ name, size = 16 }: { name: keyof typeof PATHS | string; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {PATHS[name]}
    </svg>
  );
}
