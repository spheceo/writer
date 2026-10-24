import { type CSSProperties, Fragment, type ReactNode, type RefObject, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

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

/** Open dialogs, newest last: only the top one answers Escape and keeps Tab inside it. */
const OPEN_DIALOGS: object[] = [];

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
    const me = {};
    OPEN_DIALOGS.push(me);
    const before = document.activeElement as HTMLElement | null;
    const node = box.current;
    const first = node?.querySelector<HTMLElement>("[data-autofocus]") ?? node?.querySelector<HTMLElement>("input, textarea");
    (first ?? node)?.focus();
    const onKey = (event: KeyboardEvent) => {
      // A menu open over the dialog takes its own Escape, and a dialog over this one its keys.
      if ((event.target as HTMLElement | null)?.closest?.(".menu-list") || OPEN_DIALOGS[OPEN_DIALOGS.length - 1] !== me) return;
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
      OPEN_DIALOGS.splice(OPEN_DIALOGS.indexOf(me), 1);
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

export type MenuItem = { label: string; onSelect: () => void; danger?: boolean; disabled?: boolean; hint?: string; separated?: boolean; icon?: string; checked?: boolean };

/**
 * Where a popover goes: under its trigger, or above it when there's no room
 * below, kept on screen. It lives in a portal on top of everything, so a
 * row's hover or a panel never covers it.
 */
function usePlace(shown: boolean, trigger: RefObject<HTMLElement | null>, list: RefObject<HTMLElement | null>, align: "start" | "end", matchWidth: boolean) {
  const [place, setPlace] = useState<CSSProperties>({ visibility: "hidden" });
  useLayoutEffect(() => {
    if (!shown) return;
    const put = () => {
      const anchor = trigger.current?.getBoundingClientRect();
      const box = list.current;
      if (!anchor || !box) return;
      const width = Math.max(box.offsetWidth, matchWidth ? anchor.width : 0);
      const height = box.offsetHeight;
      const below = anchor.bottom + 4 + height <= window.innerHeight - 8 || anchor.top - 4 - height < 8;
      const left = Math.min(Math.max(8, align === "end" ? anchor.right - width : anchor.left), window.innerWidth - width - 8);
      setPlace({
        top: below ? anchor.bottom + 4 : anchor.top - 4 - height,
        left,
        minWidth: matchWidth ? anchor.width : undefined,
        transformOrigin: `${below ? "top" : "bottom"} ${align === "end" ? "right" : "left"}`,
      });
    };
    put();
    window.addEventListener("resize", put);
    window.addEventListener("scroll", put, true);
    return () => {
      window.removeEventListener("resize", put);
      window.removeEventListener("scroll", put, true);
    };
  }, [shown, align, matchWidth, trigger, list]);
  return place;
}

/** The list a Menu or a Select opens: keyboard-led, placed by usePlace, closed by a click anywhere else. */
function MenuList({
  open,
  setOpen,
  items,
  head,
  trigger,
  align,
  matchWidth = false,
  className,
}: {
  open: boolean;
  setOpen: (open: boolean) => void;
  items: MenuItem[];
  head?: ReactNode;
  trigger: RefObject<HTMLElement | null>;
  align: "start" | "end";
  matchWidth?: boolean;
  className?: string;
}) {
  const [active, setActive] = useState(0);
  const { mounted, closing } = usePresence(open, 110);
  const list = useRef<HTMLDivElement>(null);
  const place = usePlace(mounted, trigger, list, align, matchWidth);

  // The list mounts a render after opening; take focus once it is there.
  useEffect(() => {
    if (open && mounted) list.current?.focus();
  }, [open, mounted]);

  useEffect(() => {
    if (!open) return;
    const checked = items.findIndex((item) => item.checked && !item.disabled);
    setActive(checked >= 0 ? checked : items.findIndex((item) => !item.disabled));
    const onDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!list.current?.contains(target) && !trigger.current?.contains(target)) setOpen(false);
    };
    window.addEventListener("pointerdown", onDown, true);
    return () => window.removeEventListener("pointerdown", onDown, true);
    // Items are rebuilt on every render; only opening resets the highlight.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const choose = (item: MenuItem | undefined) => {
    if (!item || item.disabled) return;
    setOpen(false);
    trigger.current?.focus();
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

  if (!mounted) return null;
  const marks = items.some((item) => item.checked !== undefined);
  return createPortal(
    <div
      ref={list}
      className={[closing ? "menu-list out" : "menu-list", className ? `${className}-list` : ""].join(" ")}
      role="menu"
      tabIndex={-1}
      style={place}
      onClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        if (event.key === "ArrowDown") step(1);
        else if (event.key === "ArrowUp") step(-1);
        else if (event.key === "Enter" || event.key === " ") choose(items[active]);
        else if (event.key === "Escape" || event.key === "Tab") {
          setOpen(false);
          trigger.current?.focus();
        } else return;
        event.preventDefault();
        event.stopPropagation();
      }}
    >
      {head && <div className="menu-head">{head}</div>}
      {items.map((item, index) => (
        <Fragment key={item.label}>
          {item.separated && <hr className="menu-sep" />}
          <button
            type="button"
            role={marks ? "menuitemradio" : "menuitem"}
            aria-checked={marks ? !!item.checked : undefined}
            tabIndex={-1}
            disabled={item.disabled}
            className={[index === active ? "active" : "", item.danger ? "danger" : ""].join(" ")}
            onPointerEnter={() => !item.disabled && setActive(index)}
            onClick={() => choose(item)}
          >
            {item.icon && <Icon name={item.icon} size={14} />}
            <span className="menu-label">{item.label}</span>
            {item.hint && <span className="menu-hint">{item.hint}</span>}
            {marks && <span className="menu-check">{item.checked && <Icon name="check" size={13} />}</span>}
          </button>
        </Fragment>
      ))}
    </div>,
    document.body,
  );
}

/** A menu from a button: the "more" dots by default, or what `trigger` shows (the account menu opens from your avatar, `head` above its items). */
export function Menu({ items, label = "More", trigger, head, className, align = "end" }: { items: MenuItem[]; label?: string; trigger?: ReactNode; head?: ReactNode; className?: string; align?: "start" | "end" }) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  return (
    <div className={className ? `menu ${className}` : "menu"} onClick={(event) => event.stopPropagation()}>
      <button
        ref={button}
        type="button"
        className={trigger ? (open ? "menu-trigger on" : "menu-trigger") : open ? "icon-button on" : "icon-button"}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        {trigger ?? <Icon name="more" />}
      </button>
      <MenuList open={open} setOpen={setOpen} items={items} head={head} trigger={button} align={align} className={className} />
    </div>
  );
}

/** Writer's own dropdown: the chosen option and a chevron, opening a list like a Menu's, the chosen one ticked. */
export function Select<T extends string>({
  value,
  options,
  onChange,
  label,
  disabled,
  size = "md",
  className,
  align = "start",
}: {
  value: T;
  options: { value: T; label: string; hint?: string }[];
  onChange: (value: T) => void;
  label: string;
  disabled?: boolean;
  /** md: a field's height; sm: quiet, inline in a toolbar or a line of text. */
  size?: "md" | "sm";
  className?: string;
  align?: "start" | "end";
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const current = options.find((option) => option.value === value);
  return (
    <>
      <button
        ref={button}
        type="button"
        className={["select", `select-${size}`, open ? "on" : "", className ?? ""].join(" ")}
        aria-label={label}
        title={label}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        onClick={(event) => {
          event.stopPropagation();
          setOpen(!open);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            setOpen(true);
          }
        }}
      >
        <span className="truncate">{current?.label ?? value}</span>
        <Icon name="chevron" size={size === "sm" ? 12 : 14} />
      </button>
      <MenuList
        open={open}
        setOpen={setOpen}
        trigger={button}
        align={align}
        matchWidth={size === "md"}
        items={options.map((option) => ({ label: option.label, hint: option.hint, checked: option.value === value, onSelect: () => option.value !== value && onChange(option.value) }))}
      />
    </>
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
  share: <path d="M8 2.75v7.5M5.25 5.5 8 2.75l2.75 2.75M3.75 8.5v4a.75.75 0 0 0 .75.75h7a.75.75 0 0 0 .75-.75v-4" />,
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

/**
 * Edit's own icons, drawn as Edit draws them (lucide v1.55.0, ISC: 24-unit
 * grid, 2-unit stroke), for what Writer shares with Edit: the account menu,
 * Update, New conversation.
 */
const EDIT_PATHS: Record<string, ReactNode> = {
  user: (
    <>
      <circle cx="12" cy="8" r="5" />
      <path d="M20 21a8 8 0 0 0-16 0" />
    </>
  ),
  gear: (
    <>
      <path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0 2.34 2.34 0 0 0 3.319 1.915 2.34 2.34 0 0 1 2.33 4.033 2.34 2.34 0 0 0 0 3.831 2.34 2.34 0 0 1-2.33 4.033 2.34 2.34 0 0 0-3.319 1.915 2.34 2.34 0 0 1-4.659 0 2.34 2.34 0 0 0-3.32-1.915 2.34 2.34 0 0 1-2.33-4.033 2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915" />
      <circle cx="12" cy="12" r="3" />
    </>
  ),
  logs: (
    <>
      <path d="M15 12h-5" />
      <path d="M15 8h-5" />
      <path d="M19 17V5a2 2 0 0 0-2-2H4" />
      <path d="M8 21h12a2 2 0 0 0 2-2v-1a1 1 0 0 0-1-1H11a1 1 0 0 0-1 1v1a2 2 0 1 1-4 0V5a2 2 0 1 0-4 0v2a1 1 0 0 0 1 1h3" />
    </>
  ),
  bug: (
    <>
      <path d="M12 20v-9" />
      <path d="M14 7a4 4 0 0 1 4 4v3a6 6 0 0 1-12 0v-3a4 4 0 0 1 4-4z" />
      <path d="M14.12 3.88 16 2" />
      <path d="M21 21a4 4 0 0 0-3.81-4" />
      <path d="M21 5a4 4 0 0 1-3.55 3.97" />
      <path d="M22 13h-4" />
      <path d="M3 21a4 4 0 0 1 3.81-4" />
      <path d="M3 5a4 4 0 0 0 3.55 3.97" />
      <path d="M6 13H2" />
      <path d="m8 2 1.88 1.88" />
      <path d="M9 7.13V6a3 3 0 1 1 6 0v1.13" />
    </>
  ),
  "sign-out": (
    <>
      <path d="m16 17 5-5-5-5" />
      <path d="M21 12H9" />
      <path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4" />
    </>
  ),
  reset: (
    <>
      <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
      <path d="M3 3v5h5" />
    </>
  ),
  "update-up": (
    <>
      <circle cx="12" cy="12" r="10" />
      <path d="m16 12-4-4-4 4" />
      <path d="M12 16V8" />
    </>
  ),
  "new-chat": (
    <>
      <path d="M22 17a2 2 0 0 1-2 2H6.828a2 2 0 0 0-1.414.586l-2.202 2.202A.71.71 0 0 1 2 21.286V5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2z" />
      <path d="M12 8v6" />
      <path d="M9 11h6" />
    </>
  ),
};

export function Icon({ name, size = 16 }: { name: keyof typeof PATHS | string; size?: number }) {
  const edit = EDIT_PATHS[name];
  return (
    <svg
      width={size}
      height={size}
      viewBox={edit ? "0 0 24 24" : "0 0 16 16"}
      fill="none"
      stroke="currentColor"
      strokeWidth={edit ? 2 : 1.4}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {edit ?? PATHS[name]}
    </svg>
  );
}
