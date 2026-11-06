// What happens to each HTTP or HTTPS request a sandbox makes. The container starts with
// enableInternet: false and every request on ports 80 and 443 is intercepted, so this is the
// only way out. No Cloudflare runtime imports, so it can be tested with Bun.

export const MODEL_HOST = "api.anthropic.com";

export type OutboundProps = {
  /** Hostname patterns from the create request: `github.com`, `*.npmjs.org`. */
  allow: string[];
  /** Present only while the repository is being cloned. */
  cloneToken?: string;
};

/** `*.example.com` matches subdomains of example.com, not example.com itself. */
export function hostAllowed(host: string, patterns: string[]): boolean {
  const h = host.toLowerCase().replace(/\.$/, "");
  return patterns.some((p) => (p.startsWith("*.") ? h.endsWith(p.slice(1)) && h.length > p.length - 1 : h === p));
}

function deny(status: number, why: string): Response {
  return new Response(`${why}\n`, { status, headers: { "content-type": "text/plain; charset=utf-8" } });
}

/**
 * Returns the request to send on, with credentials added where they belong, or the response
 * to give the sandbox instead. Credentials are only ever added over HTTPS, because the Worker
 * fetches with the scheme the sandbox used.
 */
export function outboundDecision(request: Request, props: OutboundProps, apiKey: string | undefined): Request | Response {
  const url = new URL(request.url);
  const host = url.hostname.toLowerCase();
  const https = url.protocol === "https:";

  if (host === MODEL_HOST) {
    if (!https) return deny(403, `${MODEL_HOST} is reachable only over HTTPS`);
    if (!apiKey) return deny(500, "ANTHROPIC_API_KEY isn't set on the Worker");
    const headers = new Headers(request.headers);
    // Whatever the sandbox sent is a placeholder at best.
    headers.delete("authorization");
    headers.set("x-api-key", apiKey);
    return new Request(request, { headers });
  }

  if (!hostAllowed(host, props.allow)) return deny(403, `${host} isn't on this sandbox's allowlist`);

  if (props.cloneToken && https && host === "github.com") {
    const headers = new Headers(request.headers);
    headers.set("authorization", `Basic ${btoa(`x-access-token:${props.cloneToken}`)}`);
    return new Request(request, { headers });
  }
  return request;
}
