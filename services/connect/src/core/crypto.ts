// Web Crypto helpers. The same API exists in Workers and in Bun, so this runs in both.

const enc = new TextEncoder();
const dec = new TextDecoder();

export function randomBytes(n: number): Uint8Array<ArrayBuffer> {
  return crypto.getRandomValues(new Uint8Array(n));
}

export function b64url(bytes: Uint8Array): string {
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromB64url(s: string): Uint8Array<ArrayBuffer> {
  const std = s.replace(/-/g, "+").replace(/_/g, "/");
  const bin = atob(std + "=".repeat((4 - (std.length % 4)) % 4));
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

function fromB64(s: string): Uint8Array<ArrayBuffer> {
  const bin = atob(s.trim());
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

/** A URL-safe random secret: `bytes` random bytes, base64url without padding. */
export function randomToken(bytes = 32): string {
  return b64url(randomBytes(bytes));
}

export async function sha256Hex(value: string): Promise<string> {
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", enc.encode(value)));
  return Array.from(digest, (b) => b.toString(16).padStart(2, "0")).join("");
}

/** Compares two strings without stopping at the first difference. */
export function constantTimeEqual(a: string, b: string): boolean {
  const x = enc.encode(a);
  const y = enc.encode(b);
  let diff = x.length ^ y.length;
  for (let i = 0; i < Math.max(x.length, y.length); i++) diff |= (x[i] ?? 0) ^ (y[i] ?? 0);
  return diff === 0;
}

const hmacKeys = new Map<string, Promise<CryptoKey>>();

function hmacKey(secret: string): Promise<CryptoKey> {
  let key = hmacKeys.get(secret);
  if (!key) {
    key = crypto.subtle.importKey("raw", enc.encode(secret), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
    hmacKeys.set(secret, key);
  }
  return key;
}

export async function hmac(secret: string, message: string): Promise<string> {
  const sig = await crypto.subtle.sign("HMAC", await hmacKey(secret), enc.encode(message));
  return b64url(new Uint8Array(sig));
}

/** `value.signature`, where the signature is HMAC-SHA256 under `secret`. */
export async function sign(secret: string, value: string): Promise<string> {
  return `${value}.${await hmac(secret, value)}`;
}

/** The value inside a string made by `sign`, or null if the signature doesn't match. */
export async function unsign(secret: string, signed: string): Promise<string | null> {
  const dot = signed.lastIndexOf(".");
  if (dot <= 0) return null;
  const value = signed.slice(0, dot);
  return constantTimeEqual(signed.slice(dot + 1), await hmac(secret, value)) ? value : null;
}

// ---- AES-GCM for secrets at rest -------------------------------------------------------

export class KeyError extends Error {}

const aesKeys = new Map<string, Promise<CryptoKey>>();

/** ENCRYPTION_KEY is 32 bytes, base64 (standard or URL-safe). */
function aesKey(encoded: string): Promise<CryptoKey> {
  let key = aesKeys.get(encoded);
  if (!key) {
    let raw: Uint8Array<ArrayBuffer>;
    try {
      raw = /[-_]/.test(encoded) ? fromB64url(encoded.trim()) : fromB64(encoded);
    } catch {
      throw new KeyError("ENCRYPTION_KEY isn't valid base64");
    }
    if (raw.length !== 32) throw new KeyError(`ENCRYPTION_KEY must be 32 bytes; it is ${raw.length}`);
    key = crypto.subtle.importKey("raw", raw, "AES-GCM", false, ["encrypt", "decrypt"]);
    aesKeys.set(encoded, key);
  }
  return key;
}

/**
 * Encrypts with AES-256-GCM and a fresh 12-byte IV. The result is `v1.<iv>.<ciphertext>`,
 * both base64url. `context` (such as the row id) is bound as additional data, so a value
 * copied into another row won't decrypt there.
 */
export async function encrypt(encodedKey: string, plaintext: string, context: string): Promise<string> {
  const iv = randomBytes(12);
  const ct = await crypto.subtle.encrypt(
    { name: "AES-GCM", iv, additionalData: enc.encode(context) },
    await aesKey(encodedKey),
    enc.encode(plaintext),
  );
  return `v1.${b64url(iv)}.${b64url(new Uint8Array(ct))}`;
}

export async function decrypt(encodedKey: string, sealed: string, context: string): Promise<string> {
  const [version, iv, ct] = sealed.split(".");
  if (version !== "v1" || !iv || !ct) throw new KeyError("unknown ciphertext format");
  const pt = await crypto.subtle.decrypt(
    { name: "AES-GCM", iv: fromB64url(iv), additionalData: enc.encode(context) },
    await aesKey(encodedKey),
    fromB64url(ct),
  );
  return dec.decode(pt);
}
