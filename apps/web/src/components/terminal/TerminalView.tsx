import { useEffect, useRef } from "react";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";

const DARK = {
  background: "#16181d",
  foreground: "#e6e8ec",
  cursor: "#8fb4ff",
  selectionBackground: "#8fb4ff55",
};
const LIGHT = {
  background: "#fcfcfd",
  foreground: "#24272e",
  cursor: "#2f6fe0",
  selectionBackground: "#2f6fe033",
  // The default palette is tuned for dark; these stay readable on white.
  white: "#6b7280",
  brightWhite: "#4b5563",
  yellow: "#a16207",
  brightYellow: "#b45309",
};
const dark = () => window.matchMedia("(prefers-color-scheme: dark)").matches;

/**
 * One PTY. Bytes flow both ways over a WebSocket; the daemon replays
 * scrollback on connect, so remounting (switching tabs) loses nothing.
 */
export function TerminalView({ socketUrl, visible }: { socketUrl: string; visible: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const termRef = useRef<XTerm | null>(null);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const term = new XTerm({
      fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
      fontSize: 12.5,
      cursorBlink: true,
      scrollback: 10_000,
      theme: dark() ? DARK : LIGHT,
    });
    const scheme = window.matchMedia("(prefers-color-scheme: dark)");
    const retheme = () => (term.options.theme = scheme.matches ? DARK : LIGHT);
    scheme.addEventListener("change", retheme);
    const fit = new FitAddon();
    term.loadAddon(fit);
    term.open(el);
    termRef.current = term;
    fitRef.current = fit;

    const ws = new WebSocket(socketUrl);
    ws.binaryType = "arraybuffer";
    const encoder = new TextEncoder();
    const sendResize = () => {
      if (ws.readyState === WebSocket.OPEN) ws.send(JSON.stringify({ type: "resize", cols: term.cols, rows: term.rows }));
    };
    ws.onopen = () => {
      safeFit(fit);
      sendResize();
    };
    ws.onmessage = (msg) => {
      term.write(typeof msg.data === "string" ? msg.data : new Uint8Array(msg.data as ArrayBuffer));
    };
    ws.onclose = () => term.write("\r\n\x1b[2m[disconnected]\x1b[0m\r\n");
    const input = term.onData((d) => {
      if (ws.readyState === WebSocket.OPEN) ws.send(encoder.encode(d));
    });
    const resized = term.onResize(sendResize);

    const observer = new ResizeObserver(() => safeFit(fit));
    observer.observe(el);

    return () => {
      scheme.removeEventListener("change", retheme);
      observer.disconnect();
      input.dispose();
      resized.dispose();
      ws.onclose = null;
      ws.close();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, [socketUrl]);

  // Hidden terminals have no size; refit and focus when shown.
  useEffect(() => {
    if (!visible || !fitRef.current) return;
    safeFit(fitRef.current);
    termRef.current?.focus();
  }, [visible]);

  return <div ref={host} className={`h-full w-full px-2 py-1.5 ${visible ? "" : "hidden"}`} />;
}

function safeFit(fit: FitAddon) {
  try {
    fit.fit();
  } catch {
    // Throws while the element is detached or hidden; the next resize retries.
  }
}
