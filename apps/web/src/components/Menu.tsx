import { useEffect, useRef, useState, type ReactNode } from "react";

/** A 16-unit line icon; pass its paths as children. */
export function Icon({ children, className = "size-4" }: { children: ReactNode; className?: string }) {
  return (
    <svg aria-hidden viewBox="0 0 16 16" className={`shrink-0 ${className}`} fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round">
      {children}
    </svg>
  );
}

/** A square icon button; the label doubles as its tooltip. */
export function IconButton({
  label,
  onClick,
  disabled,
  pressed,
  className = "",
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  pressed?: boolean;
  className?: string;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={label}
      aria-pressed={pressed}
      title={label}
      className={`flex size-7 shrink-0 items-center justify-center rounded-md hover:bg-raised hover:text-fg disabled:opacity-40 max-md:size-9 ${
        pressed ? "bg-accent/12 text-accent hover:text-accent" : "text-muted"
      } ${className}`}
    >
      <Icon className="size-3.5">{children}</Icon>
    </button>
  );
}

/**
 * A button that opens a short list of actions. Closes on a pick, a click
 * elsewhere or Escape (which returns focus to the button).
 */
export function Menu({
  label,
  trigger,
  triggerClassName = "",
  align = "right",
  side = "below",
  width = "w-60",
  children,
}: {
  label: string;
  /** What the button shows; defaults to a "more" ellipsis. */
  trigger?: ReactNode;
  triggerClassName?: string;
  align?: "left" | "right";
  side?: "below" | "above";
  width?: string;
  children: (close: () => void) => ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    const escape = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
        root.current?.querySelector<HTMLButtonElement>("button")?.focus();
      }
    };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", escape);
    root.current?.querySelector<HTMLElement>("[role=menuitem], [role=menuitemcheckbox]")?.focus();
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);
  // Whole class names, so Tailwind sees them.
  const place = {
    "below-right": "top-full mt-1 right-0 origin-top-right",
    "below-left": "top-full mt-1 left-0 origin-top-left",
    "above-right": "bottom-full mb-1 right-0 origin-bottom-right",
    "above-left": "bottom-full mb-1 left-0 origin-bottom-left",
  }[`${side}-${align}` as const];
  return (
    <div ref={root} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={open}
        title={label}
        className={
          triggerClassName ||
          `flex size-7 items-center justify-center rounded-md text-muted hover:bg-raised hover:text-fg max-md:size-9 ${open ? "bg-raised text-fg" : ""}`
        }
      >
        {trigger ?? (
          <svg aria-hidden viewBox="0 0 16 16" className="size-3.5" fill="currentColor">
            <circle cx="3.5" cy="8" r="1.2" />
            <circle cx="8" cy="8" r="1.2" />
            <circle cx="12.5" cy="8" r="1.2" />
          </svg>
        )}
      </button>
      {open && (
        <div role="menu" className={`menu-in absolute z-40 ${place} ${width} rounded-lg border border-line bg-panel p-1 text-[13px] shadow-xl shadow-black/15`}>
          {children(() => setOpen(false))}
        </div>
      )}
    </div>
  );
}

export function MenuItem({ onSelect, danger, children }: { onSelect: () => void; danger?: boolean; children: ReactNode }) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onSelect}
      className={`flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left hover:bg-raised focus-visible:bg-raised focus-visible:outline-none ${danger ? "text-bad" : ""}`}
    >
      {children}
    </button>
  );
}

export function MenuSeparator() {
  return <div role="separator" className="my-1 h-px bg-line" />;
}
