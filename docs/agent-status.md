# Agent status

A coding agent running in a Rodtty terminal can report what it's doing. Rodtty shows the report on that terminal's tab (`· Agent working`, `waiting for input`, `finished` or `unknown`) and in the command palette's sessions overview. It is separate from shell activity: a tab can be busy with the agent's process and show the agent waiting for input. Session-close protection follows the shell's running command, never the agent's report.

## Interface, version 1

Every Rodtty session sets two environment variables for the programs it runs:

- `RODTTY_AGENT_STATUS=1`, the interface version.
- `RODTTY_AGENT_STATUS_TOKEN`, a random secret unique to this session.

An agent reports by writing an escape sequence to its own terminal:

```
ESC ] 777 ; rodtty-status ; 1 ; <token> ; <sequence> ; <state> [; <ttl>] BEL
```

- `state` is `working`, `waiting` or `finished`.
- `sequence` is a number that grows with each report. A report whose sequence isn't higher than the last accepted one is ignored.
- `ttl` is how long the report holds, 1 to 3600 seconds (default 120). After it, the tab shows `unknown` until the next report.

Rodtty accepts a report only from the session whose token it carries, so writing to another terminal or printing a captured sequence changes nothing. Reports over 256 bytes, with another version, an unknown state, a wrong token or an older sequence are ignored. The status clears when the command the agent reported from exits, or with the session.

Reports stay in memory on this machine. They are not saved, synced or sent anywhere, and the interface carries no commands, input or transcripts.

## Sample

[`scripts/agent-status/rodtty-status.sh`](../scripts/agent-status/rodtty-status.sh) defines `rodtty_status STATE [TTL]` for shell wrappers; [`picobu-sample.sh`](../scripts/agent-status/picobu-sample.sh) reports each phase of a Picobu-style run.
