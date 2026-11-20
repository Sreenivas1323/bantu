import { useEffect, useId, useRef, type ReactNode } from "react";
import { afterExit } from "../lib/motion.ts";

/**
 * A modal with a fixed header and a body that scrolls on its own, so the
 * title and the close button stay put however long the content gets.
 * Escape, the close button and `close()` all play the exit before `onClose`.
 */
export function Dialog({
  title,
  onClose,
  width = 720,
  children,
}: {
  title: ReactNode;
  onClose: () => void;
  /** Widest it gets, in CSS pixels; it always leaves a margin on small screens. */
  width?: number;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    ref.current?.showModal();
  }, []);
  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      onClose={() => afterExit(onClose)}
      style={{ width: `min(${width}px, calc(100vw - 2rem))` }}
      className="m-auto max-h-[85dvh] flex-col overflow-hidden rounded-xl border border-line bg-panel p-0 text-fg shadow-2xl shadow-black/40 backdrop:bg-black/60 open:flex"
    >
      <header className="flex shrink-0 items-center justify-between gap-3 border-b border-line py-2 pr-2 pl-4">
        <h2 id={titleId} className="min-w-0 truncate font-medium">
          {title}
        </h2>
        <button
          type="button"
          onClick={() => ref.current?.close()}
          aria-label="Close"
          className="flex size-8 shrink-0 items-center justify-center rounded-md text-muted hover:bg-raised hover:text-fg"
        >
          <svg aria-hidden viewBox="0 0 16 16" className="size-3.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
            <path d="M4 4l8 8M12 4l-8 8" />
          </svg>
        </button>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">{children}</div>
    </dialog>
  );
}
