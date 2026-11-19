# Installing Rodtty

Rodtty installs as a regular desktop app: after installing, open it from Applications, the Start menu, or your Linux app launcher. You don't need a terminal to run it.

Every release on [GitHub Releases](https://github.com/CouraLabs/Rodtty/releases) contains these files:

| System | Minimum version | Architectures | File |
|---|---|---|---|
| macOS | macOS 11 Big Sur | Apple Silicon and Intel | `Rodtty-<version>-macos-universal.dmg` or `.zip` |
| Windows | Windows 10 | x64 and ARM64 | `Rodtty-<version>-windows-x64.msi`, `Rodtty-<version>-windows-arm64.msi` |
| Linux | glibc 2.35 (Ubuntu 22.04, Debian 12, Fedora 36, or newer) | x86_64 and ARM64 | `rodtty-<version>-linux-x86_64.tar.gz`, `rodtty-<version>-linux-aarch64.tar.gz` |

`SHA256SUMS.txt` in the same release lists the checksum of every file.

To build Rodtty yourself instead, see [Build and run](../README.md#build-and-run).

## macOS

### With Homebrew

```sh
brew install --cask couralabs/tap/rodtty
```

Rodtty appears in Applications and Launchpad. Update with `brew upgrade --cask rodtty` and remove with `brew uninstall --cask rodtty`. Add `--zap` to the uninstall command to also delete settings and saved sessions.

### Manual download

1. Download `Rodtty-<version>-macos-universal.dmg` from the latest release.
2. Open it and drag **Rodtty** onto **Applications**.
3. Open Rodtty from Applications or Launchpad.

If a release isn't notarized by Apple yet, macOS refuses to open it the first time and says it can't verify the developer. Either open **System Settings → Privacy & Security** and choose **Open Anyway** for Rodtty, or run this once:

```sh
xattr -dr com.apple.quarantine /Applications/Rodtty.app
```

To uninstall, move `/Applications/Rodtty.app` to the Trash. Settings live in `~/Library/Application Support/dev.CouraLabs.Rodtty`.

## Windows

### With winget

```powershell
winget install CouraLabs.Rodtty
```

winget picks the x64 or ARM64 installer for your PC. Rodtty appears in the Start menu. Update with `winget upgrade CouraLabs.Rodtty` and remove with `winget uninstall CouraLabs.Rodtty`.

### Manual download

1. Download the `.msi` for your PC from the latest release. Most PCs need `windows-x64`. Use `windows-arm64` on ARM devices such as Snapdragon laptops. **Settings → System → About → System type** tells you which one you have.
2. Run the installer and approve the administrator prompt. Rodtty installs into `C:\Program Files\Rodtty`.
3. Open Rodtty from the Start menu.

If Windows SmartScreen shows "Windows protected your PC" for an unsigned release, choose **More info → Run anyway**.

To uninstall, use **Settings → Apps → Installed apps → Rodtty → Uninstall**.

## Linux

Rodtty needs a graphics driver with Vulkan support plus the usual desktop libraries. Most desktop installations already have them. If Rodtty doesn't start, install them:

| Distribution | Command |
|---|---|
| Debian, Ubuntu | `sudo apt install libvulkan1 mesa-vulkan-drivers libxkbcommon0 fontconfig` |
| Fedora | `sudo dnf install vulkan-loader mesa-vulkan-drivers libxkbcommon fontconfig` |
| Arch | `sudo pacman -S vulkan-icd-loader libxkbcommon fontconfig`, plus the Vulkan driver for your GPU: `vulkan-intel`, `vulkan-radeon`, or `nvidia-utils` |

NVIDIA's proprietary driver includes its own Vulkan support.

### Release archive (recommended)

```sh
arch=$(uname -m)   # x86_64 or aarch64
version=0.1.0      # the release you want
curl -LO "https://github.com/CouraLabs/Rodtty/releases/download/v$version/rodtty-$version-linux-$arch.tar.gz"
tar -xzf "rodtty-$version-linux-$arch.tar.gz"
"./rodtty-$version-linux-$arch/install.sh"
```

`install.sh` installs into `~/.local` without root and adds Rodtty to your app launcher. For a system-wide install, run `sudo ./install.sh --prefix /usr/local`. To update, run the same steps with the new version. To uninstall, run `install.sh --uninstall` with the same `--prefix` you installed with.

### With Homebrew

```sh
brew install couralabs/tap/rodtty
```

Homebrew doesn't add apps to the Linux launcher by itself. To make Rodtty appear there, link its desktop entry once:

```sh
mkdir -p ~/.local/share/applications
ln -sf "$(brew --prefix)/opt/rodtty/share/applications/rodtty.desktop" ~/.local/share/applications/
```

Update with `brew upgrade rodtty` and remove with `brew uninstall rodtty`. After uninstalling, also delete `~/.local/share/applications/rodtty.desktop`.

## Verify a download

Download `SHA256SUMS.txt` from the same release into the folder that holds the file, then run:

- macOS: `shasum -a 256 -c SHA256SUMS.txt --ignore-missing`
- Linux: `sha256sum -c SHA256SUMS.txt --ignore-missing`
- Windows (PowerShell): `Get-FileHash .\Rodtty-<version>-windows-x64.msi`, then compare the hash with the matching line in `SHA256SUMS.txt`.

## Install with an AI agent

Copy this prompt into a coding agent that can run commands on your computer, such as Kiro, Claude Code, or Codex. It installs Rodtty the same way the sections above describe and asks before anything that needs administrator rights.

````text
Install the Rodtty desktop app (https://github.com/CouraLabs/Rodtty) on this computer so I can open it from my normal app launcher. Follow the official guide at https://github.com/CouraLabs/Rodtty/blob/main/docs/install.md and these rules:

1. Detect the operating system, its version, and the CPU architecture. Stop and tell me if the system is older than the minimum in the guide: macOS 11, Windows 10, or a Linux distribution with glibc 2.35 or newer (check with `ldd --version`).
2. Find the latest release with the GitHub API (https://api.github.com/repos/CouraLabs/Rodtty/releases/latest). Use only files from that release.
3. Choose the install method:
   - macOS: if Homebrew is installed, run `brew install --cask couralabs/tap/rodtty`. Otherwise download the `macos-universal.dmg`, mount it, copy Rodtty.app into /Applications, and unmount it.
   - Windows: if `winget` is available, run `winget install --exact --id CouraLabs.Rodtty`. Otherwise download the MSI that matches the architecture (x64 or arm64) and run `msiexec /i <file> /passive`.
   - Linux: download the `linux-x86_64` or `linux-aarch64` archive, extract it into a temporary folder, and run its `install.sh` without sudo, so it installs into ~/.local.
4. Before installing any file you downloaded, verify its SHA-256 checksum against SHA256SUMS.txt from the same release. Stop if it doesn't match.
5. Ask me before any command that needs sudo or administrator rights, and before changing any security setting. On macOS, if Gatekeeper blocks the app because it isn't notarized, explain this and ask me before running `xattr -dr com.apple.quarantine /Applications/Rodtty.app`. Never disable Gatekeeper or SmartScreen globally.
6. On Linux, check that a Vulkan loader, libxkbcommon, and fontconfig are installed. If any are missing, show me the install command from the guide for my distribution and ask before running it.
7. Confirm the result: Rodtty.app exists in /Applications (macOS), a Rodtty entry exists in the Start menu (Windows), or ~/.local/share/applications/rodtty.desktop exists (Linux). Then start Rodtty once and check that it opens.
8. Delete the temporary downloads and tell me which version you installed, how it was installed, and how to update and uninstall it.
````
