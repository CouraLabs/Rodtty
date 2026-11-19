# Releasing Rodtty

`.github/workflows/release.yml` builds every package on GitHub-hosted runners, publishes a GitHub Release, and updates Homebrew and winget. User-facing install steps are in [install.md](install.md).

## Release checklist

1. Confirm the `Terminal feasibility` workflow is green on `main`.
2. Set the new version in `Cargo.toml`, then run `cargo build` so `Cargo.lock` picks it up.
3. In `CHANGELOG.md`, rename `## [Unreleased]` to `## [X.Y.Z] - YYYY-MM-DD`, add a fresh empty `## [Unreleased]` above it, and update the comparison links at the bottom.
4. Commit, then tag and push:

   ```sh
   git tag -a vX.Y.Z -m "Rodtty X.Y.Z"
   git push origin main vX.Y.Z
   ```

The workflow fails early if the tag doesn't match `Cargo.toml` or the changelog has no section for the version. A version with a suffix, such as `0.2.0-beta.1`, is published as a GitHub pre-release and skips Homebrew and winget.

To test packaging without publishing, run the workflow from the Actions tab (**Run workflow**). The packages are attached to the run as artifacts.

## What each job builds

| Job | Runner | Output |
|---|---|---|
| `macos` | `macos-latest` | Universal (`aarch64` + `x86_64`) `Rodtty.app` in a `.dmg` and `.zip`, minimum macOS 11 |
| `linux` | `ubuntu-22.04`, `ubuntu-22.04-arm` | `.tar.gz` with binaries, `.desktop` entry, icon, and `install.sh`; built against glibc 2.35 |
| `windows` | `windows-latest`, `windows-11-arm` | x64 and ARM64 MSI installers (WiX 5), static CRT |
| `release` | `ubuntu-latest` | GitHub Release with all files, `SHA256SUMS.txt`, and the changelog section as notes |
| `homebrew` | `ubuntu-latest` | Commits `Casks/rodtty.rb` and `Formula/rodtty.rb` to `CouraLabs/homebrew-tap` |
| `winget` | `windows-latest` | Opens a version-update PR on `microsoft/winget-pkgs` |

The packaging scripts in `scripts/package/` also run locally. For example, `scripts/package/macos.sh 0.1.0 target/aarch64-apple-darwin/release` builds an Apple Silicon-only app.

## One-time setup

Each step is optional. When its secrets are missing, the workflow still releases and prints a warning.

### Homebrew tap

1. Create the public repository `CouraLabs/homebrew-tap` with a README, so it has a default branch.
2. Create a fine-grained personal access token limited to that repository with **Contents: Read and write**.
3. Save it as the `HOMEBREW_TAP_TOKEN` repository secret in `CouraLabs/Rodtty`.

### Apple signing and notarization

Without these secrets, the app is ad-hoc signed and users have to approve it on first launch. This step needs an Apple Developer Program membership.

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | Base64 of the exported **Developer ID Application** `.p12` (`base64 -i cert.p12 \| pbcopy`) |
| `APPLE_CERTIFICATE_PASSWORD` | The `.p12` export password |
| `APPLE_SIGNING_IDENTITY` | For example `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | Apple ID email used for notarization |
| `APPLE_TEAM_ID` | 10-character team ID |
| `APPLE_APP_PASSWORD` | App-specific password from account.apple.com |

### Windows signing with SignPath

SignPath Foundation signs open-source projects for free. After applying and creating the project in SignPath, with an artifact configuration that signs an MSI:

- Secret `SIGNPATH_API_TOKEN`: the CI user's API token.
- Variables `SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`, and `SIGNPATH_SIGNING_POLICY_SLUG`.

### winget

The workflow only updates a package that already exists in `microsoft/winget-pkgs`, so submit the first version by hand after the first release:

```powershell
winget install wingetcreate
wingetcreate new https://github.com/CouraLabs/Rodtty/releases/download/v0.1.0/Rodtty-0.1.0-windows-x64.msi https://github.com/CouraLabs/Rodtty/releases/download/v0.1.0/Rodtty-0.1.0-windows-arm64.msi
```

Use `CouraLabs.Rodtty` as the package identifier, fill in the prompts, and let it submit the PR. Once Microsoft merges it, create a classic personal access token with the `public_repo` scope and save it as the `WINGET_TOKEN` secret. wingetcreate uses it to fork `winget-pkgs` and open update PRs.
