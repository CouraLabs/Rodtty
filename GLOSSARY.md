# Rodtty

Rodtty provides a consistent terminal experience across desktop systems and remote access from browsers.

## Language

**Rodtty host**:
The computer running Rodtty where terminal commands execute and files reside.
_Avoid_: Server when referring to the whole computer, client machine

**Remote client**:
A browser or installed PWA used to access a Rodtty host from another device.
_Avoid_: Mobile app, mobile terminal host

**Bundled command**:
A command distributed with Rodtty as part of its consistent command environment.
_Avoid_: Host command

**Host command**:
A program found through the Rodtty host's inherited PATH, retaining its native availability and behavior.
_Avoid_: Bundled command, portable command

**Connection method**:
The route a remote client uses to reach a Rodtty host.

**Terminal session**:
A running command environment on a Rodtty host, with its own working directory, environment, processes, and terminal output. A session remains running when its desktop window closes or its remote client disconnects, until explicitly ended or the running host application stops.
_Avoid_: Connection, terminal tab when referring to the running environment

**Owner**:
The person whose operating-system account runs Rodtty and whose devices access its terminal sessions.

**Terminal tab**:
A view of one terminal session in the desktop application. Each tab's session has an independent current directory, environment, and running processes.
_Avoid_: Editor tab, connection

**Terminal title**:
The text supplied by a running application to name its terminal session. It takes precedence over Rodtty's directory-based fallback tab label.
_Avoid_: Window title when referring to the title of one terminal session

**Follow Terminal**:
An optional explorer mode that expands and reveals the active terminal's current directory within the explorer root. Moving outside that root offers an action to change the root instead of replacing it automatically; with this mode off, the explorer navigates independently.

**Explorer root**:
The chosen host folder at the top of Rodtty's file tree, whose descendants can be expanded and browsed without changing that folder.

**Preview editor tab**:
A temporary editor tab opened by previewing a file and reused when another file is previewed. Editing the file or double-clicking it keeps it open as a regular editor tab.

**Editor tab**:
A view of a file being edited within Rodtty.
_Avoid_: Terminal tab

**Command configuration**:
The owner's selection of available bundled utilities. Each terminal session keeps the selection that applied when that session was created.

**Git diff mode**:
The editor's read-only view for comparing current file contents with committed or staged versions and inspecting saved or unsaved changes.
_Avoid_: Git client when referring only to change inspection
