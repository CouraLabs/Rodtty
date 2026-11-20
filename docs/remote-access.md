# Remote session handoff

A trusted device's browser can reconnect to a terminal session on a running Rodtty host. Access is off until the owner turns it on.

## Transport

The first release supports one connection method: [Tailscale](https://tailscale.com). Rodtty listens on this machine's Tailscale IPv4 address (from `tailscale ip -4`), port 7681, and nowhere else. Traffic between the device and the host travels inside the tailnet's WireGuard tunnel, which encrypts it end to end; Rodtty never listens on a LAN or public interface. When Tailscale isn't running, **Remote: Enable Access over Tailscale** reports that and stays off. LAN and Cloudflare access are not offered.

`RODTTY_REMOTE_LOOPBACK=1` makes Rodtty listen on `127.0.0.1` on a random port instead. It exists for local checks on the host itself.

## Pair a device

1. Install Tailscale on the host and the device and sign both into the same tailnet.
2. In Rodtty, run **Remote: Pair Device…** from the command palette. It turns access on if needed and shows an address and an eight-character code.
3. On the device, open the address in a browser and enter the code.

A code works once and expires after five minutes; a wrong code also uses it up. Pairing gives the device a random 256-bit token from the operating system, stored in the browser. A device token expires after 30 days. Paired devices are listed in `<app root>/remote-devices.json` (mode 0600).

## Use a session

The page lists the host's sessions. **Attach** shows the session's recent output (up to 64 KiB) and keeps reading new output. A temporary connection loss is retried; the page resumes from its last position and says when output in between is no longer retained.

Control policy: every paired device can watch an attached session; only the session's controller types and resizes it. A device presses **Take control** to type. When the owner types or pastes into that session on the desktop, the desktop takes control back and the device must take control again. A remote resize applies until the desktop's own layout next resizes the terminal.

## Lifetime

Sessions run on the host. A browser disconnect leaves the shell running, and so does closing the desktop window (see [ADR 0004](adr/0004-separate-session-lifetime-from-window-lifetime.md)). **Quit** ends the sessions and remote access with them.

## Turn it off

- **Remote: Disable Access** stops listening. Paired devices stay paired for the next time access is on.
- **Remote: Disconnect and Revoke All Devices** removes every pairing; the next request from any device is refused.
- The footer shows `Remote: on` or the number of attached devices while access is on.

The first release covers one owner and the existing terminal sessions. There are no file APIs, public links, unauthenticated access or multi-user sharing. Pairing codes, tokens and terminal output are not written to logs or diagnostics.
