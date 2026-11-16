# Separate session lifetime from window lifetime

By default, closing a Rodtty desktop window leaves terminal sessions running, and reopening the application restores access to them. Closing a terminal tab ends that session; Quit Rodtty stops all sessions. Computer restarts begin fresh sessions rather than attempting to restore running processes.

This requires terminal-session ownership to outlive individual desktop windows, supporting long-running local commands and later browser reconnection. A window or network connection is a view of a terminal session rather than the owner of its process lifetime. The specific background-process and window-management implementation remains to be validated.

The UI redesign extends the original fixed window-close rule with owner-configurable action behavior: Keep sessions running (default), Ask each time, or Quit Rodtty. Terminal-tab closure defaults to confirmation when commands are running, with Always confirm and Close immediately as alternatives. Explicit Quit Rodtty continues to confirm before stopping sessions with running commands; selecting Quit Rodtty as the window-close action uses that same quit flow.

Startup restores the terminal-tab layout and reconnects to surviving sessions. After the host application stops or the computer restarts, the restored terminal tabs create fresh shells in their last directories rather than attempting to recover prior processes; the owner can instead choose to start with one fresh tab. Tab labels follow the running application's terminal title when present and otherwise use the current-directory fallback; restored workspace state does not override a running application's title.
