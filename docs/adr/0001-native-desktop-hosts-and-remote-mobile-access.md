# Native desktop hosts and remote mobile access

Rodtty targets native desktop applications on Windows, macOS, and Linux using Rust and Rune. Mobile devices access a Rodtty host through a browser or installed PWA; there will be no native mobile application. This keeps execution on the desktop host while making its terminal available remotely. Users select LAN, Tailscale, or Cloudflare access in the application's configuration; the provisioning and authentication model remains undecided.

The terminal must support interactive applications, including full-screen terminal programs. The cross-platform consistency promise covers bundled commands and shell syntax. Host commands inherited through PATH retain their native behavior and platform requirements. Bundled utilities use uutils as recorded in ADR-0002. ADR-0003 defines the shell's syntax target; the shell implementation and exact compatibility limits remain undecided.

Delivery starts with the desktop application; browser/PWA access and tunnel integrations follow later. Remote access initially serves one owner using trusted devices under the host's operating-system account. Terminal sessions remain running when a browser disconnects, allowing reconnection to the same session and recent output while the Rodtty host remains running. ADR-0004 defines desktop-window closure and host restart behavior.
