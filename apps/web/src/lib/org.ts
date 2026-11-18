import type { ClaudeAccessMode, Org, Tracker } from "@bantu/protocol";

/** What the org's Claude setting means for the member, in plain words. */
export function claudeGuidance(mode: ClaudeAccessMode, plan: string | null): { title: string; body: string } {
  switch (mode) {
    case "own":
      return {
        title: "Your own Claude account",
        body: "Claude Code uses whatever account it's signed in to on this machine, as before.",
      };
    case "team":
      return {
        title: plan ? `A seat on ${plan}` : "A seat on your organization's Claude plan",
        body: "Sign in to Claude below with your work account (Claude subscription or SSO), so chats use the team's plan instead of a personal one.",
      };
    case "api_key":
      return {
        title: "Your organization's API key",
        body: "Bantu gives Claude Code the organization's Anthropic API key, so you don't need to sign in. Usage is billed to the organization.",
      };
    case "bedrock":
      return {
        title: "Amazon Bedrock",
        body: "Claude Code reaches Claude through your organization's Amazon Bedrock account, so you don't need to sign in.",
      };
    case "vertex":
      return {
        title: "Google Vertex AI",
        body: "Claude Code reaches Claude through your organization's Google Vertex AI project, so you don't need to sign in.",
      };
  }
}

/** The org's trackers, so the app can mark them and hide their remove button. */
export function orgTrackers(trackers: Tracker[], org: Org | null): Tracker[] {
  return org ? trackers.filter((t) => t.org_id === org.id) : [];
}

/** "Synced 10:42" or the error, for the Organization section. */
export function syncLine(org: Org): { text: string; failed: boolean } {
  if (org.last_error) return { text: org.last_error, failed: true };
  if (!org.last_synced_at) return { text: "Not synced yet", failed: false };
  return { text: `Synced ${new Date(org.last_synced_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`, failed: false };
}
