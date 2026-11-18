// The signed-in session, the OAuth state cookie and CSRF tokens. All three are HMAC-SHA256
// under SESSION_SECRET. Cookies are HttpOnly, Secure and SameSite=Lax, and use the __Host-
// prefix so they're bound to this origin with Path=/.

import { b64url, constantTimeEqual, fromB64url, hmac, sign, unsign } from "./crypto.ts";

export const SESSION_COOKIE = "__Host-bantu_session";
export const STATE_COOKIE = "__Host-bantu_oauth";
export const SESSION_TTL_MS = 30 * 24 * 60 * 60 * 1000;
export const STATE_TTL_MS = 10 * 60 * 1000;

export type Session = {
  /** GitHub user id. */
  uid: number;
  /** Random per sign-in; CSRF tokens are derived from it. */
  sid: string;
  /** Expiry, Unix ms. */
  exp: number;
};

const enc = new TextEncoder();
const dec = new TextDecoder();

export function parseCookies(header: string | null): Map<string, string> {
  const out = new Map<string, string>();
  for (const part of (header ?? "").split(";")) {
    const eq = part.indexOf("=");
    if (eq < 0) continue;
    const name = part.slice(0, eq).trim();
    if (name && !out.has(name)) out.set(name, part.slice(eq + 1).trim());
  }
  return out;
}

export function cookie(name: string, value: string, maxAgeSeconds: number): string {
  return `${name}=${value}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=${maxAgeSeconds}`;
}

export function clearCookie(name: string): string {
  return cookie(name, "", 0);
}

async function signJson(secret: string, data: unknown): Promise<string> {
  return sign(secret, b64url(enc.encode(JSON.stringify(data))));
}

async function unsignJson<T>(secret: string, value: string | undefined): Promise<T | null> {
  if (!value) return null;
  const inner = await unsign(secret, value);
  if (!inner) return null;
  try {
    return JSON.parse(dec.decode(fromB64url(inner))) as T;
  } catch {
    return null;
  }
}

export async function sessionCookie(secret: string, session: Session): Promise<string> {
  return cookie(SESSION_COOKIE, await signJson(secret, session), Math.floor(SESSION_TTL_MS / 1000));
}

export async function readSession(secret: string, request: Request, now: number): Promise<Session | null> {
  const s = await unsignJson<Session>(secret, parseCookies(request.headers.get("cookie")).get(SESSION_COOKIE));
  if (!s || typeof s.uid !== "number" || typeof s.sid !== "string" || typeof s.exp !== "number" || s.exp <= now) return null;
  return s;
}

// ---- OAuth state -----------------------------------------------------------------------

export type OAuthState = { state: string; next: string; exp: number };

export async function stateCookie(secret: string, st: OAuthState): Promise<string> {
  return cookie(STATE_COOKIE, await signJson(secret, st), Math.floor(STATE_TTL_MS / 1000));
}

/** True when the callback's `state` matches the signed, unexpired cookie set at the start. */
export async function checkState(secret: string, request: Request, given: string | null, now: number): Promise<OAuthState | null> {
  const st = await unsignJson<OAuthState>(secret, parseCookies(request.headers.get("cookie")).get(STATE_COOKIE));
  if (!st || !given || typeof st.state !== "string" || st.exp <= now) return null;
  return constantTimeEqual(st.state, given) ? st : null;
}

/** A same-origin path to return to after sign-in; anything else becomes `/orgs`. */
export function safeNext(next: string | null | undefined): string {
  if (!next || !next.startsWith("/") || next.startsWith("//") || next.includes("\\") || /[\u0000-\u001f]/.test(next)) {
    return "/orgs";
  }
  return next;
}

// ---- CSRF ------------------------------------------------------------------------------

/** Every form carries this. It's bound to the session, so it changes at each sign-in. */
export function csrfToken(secret: string, session: Session): Promise<string> {
  return hmac(secret, `csrf:${session.sid}`);
}

export async function checkCsrf(secret: string, session: Session, given: string | undefined): Promise<boolean> {
  return !!given && constantTimeEqual(given, await csrfToken(secret, session));
}
