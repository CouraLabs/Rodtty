# Use Bash-style syntax across hosts

Rodtty's default shell environment will use the same Bash-style syntax across Windows, macOS, and Linux, including pipelines, redirection, quoting, variables, conditional command chains, and globbing. The first release will not promise complete compatibility with existing Bash scripts or dotfiles. This prioritizes a consistent interactive experience over adopting each host's default shell or requiring a complete Unix compatibility environment on Windows.

Brush is the initial Rust shell candidate to evaluate, not a validated dependency choice. Windows compatibility must be demonstrated before committing to it. Its interaction with terminal input, full-screen applications, command resolution, and bundled utilities remains to be validated.

New terminal tabs inherit the active terminal's directory, but begin with the host environment plus Rodtty's own startup configuration rather than copying environment mutations from another tab. The first session uses an explicitly supplied directory, otherwise the launcher's working directory, falling back to the owner's home directory. Rodtty loads its own startup file; users can explicitly source other shell configuration files. Host Bash or Zsh dotfiles are not loaded automatically, preserving a consistent default environment across systems.
