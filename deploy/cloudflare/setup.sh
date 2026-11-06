#!/bin/sh
# Puts a Bantu daemon on this machine behind Cloudflare: a Tunnel with two
# hostnames (the app and its previews), each behind Cloudflare Access.
#
#   deploy/cloudflare/setup.sh --app bantu.example.com --preview preview.bantu.example.com \
#     --email you@example.com
#
# Needs `cloudflared`, a domain on Cloudflare, and for Access an API token with
# "Access: Apps and Policies Write" and "Access: Organizations Read":
#   CLOUDFLARE_API_TOKEN=… CLOUDFLARE_ACCOUNT_ID=…
# See docs/CLOUDFLARE.md. Access is created before any DNS, so the hostnames are
# never reachable without a sign-in.
set -eu

APP="" PREVIEW="" TUNNEL="bantu" EMAILS="" NO_ACCESS=0
while [ $# -gt 0 ]; do
  case "$1" in
    --app) APP="$2"; shift 2 ;;
    --preview) PREVIEW="$2"; shift 2 ;;
    --tunnel) TUNNEL="$2"; shift 2 ;;
    --email) EMAILS="$EMAILS $2"; shift 2 ;;
    --no-access) NO_ACCESS=1; shift ;;
    -h|--help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
die() { echo "error: $*" >&2; exit 1; }
[ -n "$APP" ] && [ -n "$PREVIEW" ] || die "--app and --preview are both required"
[ "$APP" != "$PREVIEW" ] || die "the preview hostname must differ from the app's, so previewed pages can't reach the daemon's token"
command -v cloudflared >/dev/null || die "install cloudflared first: brew install cloudflared (macOS) or see https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/"
command -v python3 >/dev/null || die "python3 is needed to read Cloudflare's replies"
json() { python3 -c "import json,sys; d=json.load(sys.stdin); print($1)"; }

# --- Access, first --------------------------------------------------------
TEAM="" AUDS=""
if [ "$NO_ACCESS" = 0 ]; then
  [ -n "${CLOUDFLARE_API_TOKEN:-}" ] && [ -n "${CLOUDFLARE_ACCOUNT_ID:-}" ] ||
    die "set CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID for Access, or pass --no-access to rely on the daemon's token alone"
  [ -n "$EMAILS" ] || die "--email is needed: who may sign in"
  API="${CLOUDFLARE_API_BASE:-https://api.cloudflare.com/client/v4}/accounts/$CLOUDFLARE_ACCOUNT_ID/access"
  cf() { # method path [body]
    out=$(curl -sS -X "$1" "$API$2" -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
      -H "Content-Type: application/json" ${3:+--data "$3"})
    echo "$out" | json "'' if d.get('success') else sys.exit('Cloudflare said: ' + json.dumps(d.get('errors')))" >/dev/null
    echo "$out"
  }
  TEAM=$(cf GET /organizations | json "d['result']['auth_domain']")
  include=$(python3 -c "import json,sys; print(json.dumps([{'email': {'email': e}} for e in sys.argv[1:]]))" $EMAILS)
  POLICY=$(cf POST /policies "{\"name\":\"Bantu ($TUNNEL)\",\"decision\":\"allow\",\"include\":$include}" | json "d['result']['id']")
  for host in "$APP" "$PREVIEW"; do
    aud=$(cf POST /apps "{\"name\":\"Bantu $host\",\"domain\":\"$host\",\"type\":\"self_hosted\",\"session_duration\":\"24h\",\"policies\":[{\"id\":\"$POLICY\",\"precedence\":1}]}" | json "d['result']['aud']")
    AUDS="${AUDS:+$AUDS,}$aud"
    echo "Access protects $host"
  done
fi

# --- Tunnel ---------------------------------------------------------------
[ -f "$HOME/.cloudflared/cert.pem" ] || cloudflared tunnel login
if ! cloudflared tunnel info "$TUNNEL" >/dev/null 2>&1; then
  cloudflared tunnel create "$TUNNEL"
fi
ID=$(cloudflared tunnel list --name "$TUNNEL" --output json | json "d[0]['id']")
for host in "$APP" "$PREVIEW"; do
  # Never overwrites an existing DNS record; that would be your call.
  cloudflared tunnel route dns "$TUNNEL" "$host" || echo "note: $host already has a DNS record; point it at $ID.cfargotunnel.com yourself if it isn't already"
done

CONFIG="$HOME/.cloudflared/config.yml"
[ -f "$CONFIG" ] && CONFIG="$HOME/.cloudflared/bantu-$TUNNEL.yml"
cat > "$CONFIG" <<YML
tunnel: $ID
credentials-file: $HOME/.cloudflared/$ID.json
ingress:
  - hostname: $APP
    service: http://127.0.0.1:47800
  - hostname: $PREVIEW
    service: http://127.0.0.1:47801
  - service: http_status:404
YML
echo "Wrote $CONFIG"
if [ "$CONFIG" = "$HOME/.cloudflared/config.yml" ]; then
  cloudflared service install || echo "Start the tunnel with: cloudflared tunnel run $TUNNEL"
else
  echo "You already had a config.yml, so start this tunnel with: cloudflared tunnel --config $CONFIG run"
fi

# --- The daemon -------------------------------------------------------------
echo
echo "Now (re)install the daemon with its public addresses:"
echo
echo "  BANTU_PUBLIC_URL=https://$APP BANTU_PREVIEW_URL=https://$PREVIEW \\"
[ -n "$TEAM" ] && echo "  BANTU_CF_ACCESS_TEAM=$TEAM BANTU_CF_ACCESS_AUD=$AUDS \\"
echo "  bantud service install"
echo
echo "Then pair a phone with: bantud url --qr"
