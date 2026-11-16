# Use uutils for bundled utilities

Rodtty will use uutils, a Rust implementation targeting GNU Coreutils compatibility, rather than require the original GNU Coreutils implementation on every platform. This supports a native Rust application across Windows, macOS, and Linux without choosing a Unix compatibility environment as the default Windows foundation.

The compatibility promise is a documented supported command set with per-command compatibility validation, rather than identical GNU/Linux behavior on every host. Some utilities or operating-system capabilities are platform-specific. The default set consists of tested cross-platform commands, configurable per utility. The precise command list and unsupported-operation behavior remain to be decided.

Enabled bundled external utilities take precedence over matching executables on the inherited host PATH. Disabling a bundled utility makes its bare command name report that it is disabled, rather than silently falling back to a host version. Explicit paths to host executables remain usable, and shell built-ins are a separate category. This configures Rodtty's command environment; it does not restrict what arbitrary host applications can execute. The shell must be selected separately.

The owner edits command availability through an application settings screen. Changes apply to newly created terminal sessions; existing sessions retain the selection with which they started.
