# Rodtty's Trash adapter patch

Upstream: `trash` 5.3.0, crates.io release, MIT (see LICENSE.txt).
All Linux/Freedesktop and macOS adapter code is unchanged. Rodtty explicitly
selects NSFileManager on macOS, avoiding Finder automation permissions.

Windows recycling is restricted in `src/windows.rs`:

- Request `FOFX_RECYCLEONDELETE`, `FOFX_EARLYFAILURE`, `FOFX_ADDUNDORECORD`,
  `FOF_NO_UI` and `FOF_WANTNUKEWARNING`.
- Veto `PreDeleteItem` when the Shell callback lacks recycle intent
  (`TSF_DELETE_RECYCLE_IF_POSSIBLE`), rather than accepting a permanent fallback.
- Treat a successful `PostDeleteItem` with no recycled destination as a failure,
  never as Trash success. The application retains buffers and reports uncertainty
  if a platform error is accompanied by a changed or missing source.

The application offers permanent deletion through a separate, version-checked
worker request after an explicit owner decision. That path does not call this
adapter. Native Windows execution remains pending a disposable Windows account.

Primary API contracts:

- https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperation-setoperationflags
- https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperationprogresssink-predeleteitem
- https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-ifileoperationprogresssink-postdeleteitem
