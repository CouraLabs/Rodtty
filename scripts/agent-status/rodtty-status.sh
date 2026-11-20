# Rodtty agent status, interface version 1. Source this file in an agent's
# wrapper (Picobu or any other) and call `rodtty_status working|waiting|finished [ttl]`.
# Outside a Rodtty terminal it does nothing.
__rodtty_status_seq=0
rodtty_status() {
  [ "${RODTTY_AGENT_STATUS:-}" = 1 ] && [ -n "${RODTTY_AGENT_STATUS_TOKEN:-}" ] || return 0
  __rodtty_status_seq=$((__rodtty_status_seq + 1))
  if [ -n "${2:-}" ]; then
    printf '\033]777;rodtty-status;1;%s;%s;%s;%s\007' "$RODTTY_AGENT_STATUS_TOKEN" "$__rodtty_status_seq" "$1" "$2" > /dev/tty
  else
    printf '\033]777;rodtty-status;1;%s;%s;%s\007' "$RODTTY_AGENT_STATUS_TOKEN" "$__rodtty_status_seq" "$1" > /dev/tty
  fi
}
