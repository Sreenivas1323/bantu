import { describe, expect, test } from "bun:test";

import { constantTimeEqual, decrypt, encrypt, KeyError, randomToken, sha256Hex, sign, unsign } from "../src/core/crypto.ts";
import { hashToken, newDaemonToken } from "../src/core/daemons.ts";
import { esc, html } from "../src/core/html.ts";
import { ENV } from "./helpers.ts";

const KEY = ENV.ENCRYPTION_KEY!;

describe("token hashing", () => {
  test("daemon tokens are 32 random bytes and hash to SHA-256 hex", async () => {
    const t = newDaemonToken();
    expect(Buffer.from(t, "base64url")).toHaveLength(32);
    expect(newDaemonToken()).not.toBe(t);
    const h = await hashToken(t);
    expect(h).toMatch(/^[0-9a-f]{64}$/);
    expect(h).toBe(new Bun.CryptoHasher("sha256").update(t).digest("hex"));
    expect(await sha256Hex("abc")).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
  });

  test("randomToken length and alphabet", () => {
    expect(randomToken(24)).toMatch(/^[A-Za-z0-9_-]{32}$/);
  });

  test("constant-time comparison", () => {
    expect(constantTimeEqual("abc", "abc")).toBe(true);
    expect(constantTimeEqual("abc", "abd")).toBe(false);
    expect(constantTimeEqual("abc", "abcd")).toBe(false);
  });

  test("signed values reject tampering", async () => {
    const s = await sign("secret", "hello");
    expect(await unsign("secret", s)).toBe("hello");
    expect(await unsign("other", s)).toBeNull();
    expect(await unsign("secret", s.replace("hello", "hellp"))).toBeNull();
    expect(await unsign("secret", "hello")).toBeNull();
  });
});

describe("encryption at rest", () => {
  test("round trips, with a fresh IV each time", async () => {
    const a = await encrypt(KEY, "glpat-secret", "connection:1");
    const b = await encrypt(KEY, "glpat-secret", "connection:1");
    expect(a).not.toBe(b);
    expect(a).not.toContain("glpat-secret");
    expect(a.split(".")).toHaveLength(3);
    expect(await decrypt(KEY, a, "connection:1")).toBe("glpat-secret");
    expect(await decrypt(KEY, b, "connection:1")).toBe("glpat-secret");
  });

  test("a value moved to another row, or a different key, doesn't decrypt", async () => {
    const a = await encrypt(KEY, "x", "connection:1");
    await expect(decrypt(KEY, a, "connection:2")).rejects.toThrow();
    const other = Buffer.alloc(32, 9).toString("base64");
    await expect(decrypt(other, a, "connection:1")).rejects.toThrow();
    const [v, iv, ct] = a.split(".");
    const flipped = `${v}.${iv}.${ct!.slice(0, -2)}${ct!.endsWith("AA") ? "BB" : "AA"}`;
    await expect(decrypt(KEY, flipped, "connection:1")).rejects.toThrow();
  });

  test("the key must be 32 bytes of base64", async () => {
    await expect(encrypt(Buffer.alloc(16).toString("base64"), "x", "c")).rejects.toBeInstanceOf(KeyError);
    await expect(encrypt("not base64!!", "x", "c")).rejects.toBeInstanceOf(KeyError);
    // URL-safe base64 is accepted too.
    const urlSafe = Buffer.alloc(32, 250).toString("base64url");
    expect(await decrypt(urlSafe, await encrypt(urlSafe, "ok", "c"), "c")).toBe("ok");
  });
});

describe("html", () => {
  test("escapes every interpolation", () => {
    const evil = `<script>alert("x")</script>' onload='`;
    expect(html`<p title="${evil}">${evil}</p>`.value).toBe(`<p title="${esc(evil)}">${esc(evil)}</p>`);
    expect(html`${[html`<b>${"<i>"}</b>`, null, false, 0]}`.value).toBe("<b>&lt;i&gt;</b>0");
  });
});
