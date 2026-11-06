# The image @cloudflare/sandbox documents: any Linux image plus the sandbox-shim that its Files
# class runs, copied from the cloudflare/sandbox donor image. Keep the tag equal to the
# installed @cloudflare/sandbox version (package.json).
ARG SANDBOX_TOOLS_IMAGE=docker.io/cloudflare/sandbox:1.0.3
FROM ${SANDBOX_TOOLS_IMAGE} AS sandbox-tools

FROM node:26-bookworm-slim
# bash runs every command; ca-certificates lets the interception CA be added to the system store.
RUN apt-get update \
  && apt-get install -y --no-install-recommends bash ca-certificates git ripgrep \
  && rm -rf /var/lib/apt/lists/*
# Pinned, because burst.ts reads Claude Code's stream-json output. Its postinstall copies the
# native binary into place, so don't skip scripts.
RUN npm install --global @anthropic-ai/claude-code@2.1.318
COPY --from=sandbox-tools /usr/local/bin/sandbox-shim /usr/local/bin/sandbox-shim
WORKDIR /workspace
CMD ["sleep", "infinity"]
