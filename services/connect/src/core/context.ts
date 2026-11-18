import type { Db } from "./db.ts";

/** The Worker's secrets and vars. See wrangler.jsonc and docs/CONNECT.md. */
export type Secrets = {
  GITHUB_CLIENT_ID?: string;
  GITHUB_CLIENT_SECRET?: string;
  /** Any long random string. Signs the session, OAuth state and CSRF tokens. */
  SESSION_SECRET?: string;
  /** 32 bytes, base64. Encrypts tracker tokens and the org's API key. */
  ENCRYPTION_KEY?: string;
  /** The public origin, when it isn't the request's own (behind a proxy, say). */
  PUBLIC_URL?: string;
};

/** Everything a request handler needs, so tests can swap each piece. */
export type Deps = {
  db: Db;
  env: Secrets;
  /** Used only to reach GitHub during sign-in. Tests pass a stub. */
  fetch: typeof fetch;
  /** Unix milliseconds. */
  now: () => number;
};

export class HttpError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

export function required(env: Secrets, name: keyof Secrets): string {
  const value = env[name];
  if (!value) throw new HttpError(500, `${name} isn't set on this Worker. See docs/CONNECT.md.`);
  return value;
}
