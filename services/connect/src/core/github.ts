// Sign-in with a GitHub OAuth app (the web application flow):
// https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps
// No scope is requested, so the token can only read public profile information. It's used
// once to read the user's id and login, then dropped.

import { HttpError } from "./context.ts";
import type { User } from "./orgs.ts";

export const AUTHORIZE_URL = "https://github.com/login/oauth/authorize";
export const TOKEN_URL = "https://github.com/login/oauth/access_token";
export const USER_URL = "https://api.github.com/user";

export function authorizeUrl(clientId: string, redirectUri: string, state: string): string {
  const u = new URL(AUTHORIZE_URL);
  u.searchParams.set("client_id", clientId);
  u.searchParams.set("redirect_uri", redirectUri);
  u.searchParams.set("state", state);
  u.searchParams.set("allow_signup", "true");
  return u.toString();
}

export async function exchangeForUser(
  fetcher: typeof fetch,
  clientId: string,
  clientSecret: string,
  code: string,
  redirectUri: string,
): Promise<User> {
  const tokenRes = await fetcher(TOKEN_URL, {
    method: "POST",
    headers: { accept: "application/json", "user-agent": "bantu-connect" },
    body: new URLSearchParams({ client_id: clientId, client_secret: clientSecret, code, redirect_uri: redirectUri }),
  });
  const tokenBody = (await tokenRes.json().catch(() => ({}))) as { access_token?: string; error?: string };
  if (!tokenRes.ok || !tokenBody.access_token) {
    throw new HttpError(400, `GitHub didn't accept the sign-in${tokenBody.error ? ` (${tokenBody.error})` : ""}. Try again.`);
  }
  const userRes = await fetcher(USER_URL, {
    headers: {
      accept: "application/vnd.github+json",
      authorization: `Bearer ${tokenBody.access_token}`,
      "user-agent": "bantu-connect",
      "x-github-api-version": "2022-11-28",
    },
  });
  const u = (await userRes.json().catch(() => ({}))) as { id?: unknown; login?: unknown; name?: unknown };
  if (!userRes.ok || typeof u.id !== "number" || typeof u.login !== "string") {
    throw new HttpError(502, "GitHub didn't return your profile. Try again.");
  }
  return { github_id: u.id, login: u.login, name: typeof u.name === "string" ? u.name : null };
}
