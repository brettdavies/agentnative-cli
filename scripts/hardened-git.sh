#!/usr/bin/env bash
# Sourced by the scripts that read agentnative-site. The same hardening
# src/skill_install.rs applies to `anc skill install`: user and system git config
# disabled, credential prompts off, SSH/proxy/askpass/exec-path overrides
# stripped, and the five `-c` flags below. tests/sync_web_audit.rs pins that
# surface.

GIT_HARDEN_FLAGS=(
  -c credential.helper=
  -c core.askPass=
  -c protocol.allow=never
  -c protocol.https.allow=always
  -c http.followRedirects=false
)

hardened_git() {
  env -u GIT_SSH -u GIT_SSH_COMMAND -u GIT_PROXY_COMMAND -u GIT_ASKPASS -u GIT_EXEC_PATH \
    GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null GIT_TERMINAL_PROMPT=0 \
    git "${GIT_HARDEN_FLAGS[@]}" "$@"
}
