import type { DaemonClient } from "./api.ts";

/** Whether this browser can get Web Push alerts on this page. */
export function pushSupported(): boolean {
  return "serviceWorker" in navigator && "PushManager" in window && typeof Notification !== "undefined";
}

function decodeKey(base64url: string): Uint8Array<ArrayBuffer> {
  const raw = atob(base64url.replaceAll("-", "+").replaceAll("_", "/"));
  const bytes = new Uint8Array(new ArrayBuffer(raw.length));
  for (let i = 0; i < raw.length; i += 1) bytes[i] = raw.charCodeAt(i);
  return bytes;
}

export async function currentPushSubscription(): Promise<PushSubscription | null> {
  const registration = await navigator.serviceWorker.getRegistration();
  return (await registration?.pushManager.getSubscription()) ?? null;
}

/** Asks for permission if needed, subscribes, and tells the daemon. */
export async function enablePush(client: DaemonClient): Promise<void> {
  if ((await Notification.requestPermission()) !== "granted") {
    throw new Error("Notifications are blocked for this site in your browser's settings.");
  }
  const registration = await navigator.serviceWorker.ready;
  const { public_key } = await client.pushKey();
  const subscription = await registration.pushManager.subscribe({
    userVisibleOnly: true,
    applicationServerKey: decodeKey(public_key),
  });
  const { endpoint, keys } = subscription.toJSON();
  if (!endpoint || !keys?.p256dh || !keys.auth) throw new Error("The browser returned an incomplete subscription.");
  await client.send({ type: "push.subscribe", endpoint, p256dh: keys.p256dh, auth: keys.auth });
}

export async function disablePush(client: DaemonClient): Promise<void> {
  const subscription = await currentPushSubscription();
  if (!subscription) return;
  await client.send({ type: "push.unsubscribe", endpoint: subscription.endpoint });
  await subscription.unsubscribe();
}
