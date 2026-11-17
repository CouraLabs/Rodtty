# vscode-icons artwork and associations

Upstream: https://github.com/vscode-icons/vscode-icons
Revision: `814aab8318900f47efeae6f1c4691494ae764199`
Copyright: The vscode-icons Contributors and the respective branded artwork owners.

The SVG files in `icons/` are unmodified upstream artwork. Rodtty's
`adaptive/default_file.svg` derivative retains the original generic document path
and replaces its fixed pale gray fill with `currentColor` for contrast on both
light and dark backgrounds; this derivative remains under CC BY-SA 4.0. They are distributed
under **Creative Commons Attribution-ShareAlike 4.0 International (CC BY-SA 4.0)**,
except branded icons, which retain their respective copyright licenses. The
upstream license statement is at:
https://github.com/vscode-icons/vscode-icons/blob/814aab8318900f47efeae6f1c4691494ae764199/README.md#license

CC BY-SA 4.0 terms: https://creativecommons.org/licenses/by-sa/4.0/
Legal code: https://creativecommons.org/licenses/by-sa/4.0/legalcode.en

Attribution: vscode-icons, by the vscode-icons Contributors,
https://github.com/vscode-icons/vscode-icons . No endorsement is implied.
Brand names, trademarks, logos, and their applicable rights belong to their
respective owners; the source-code MIT license does not relicense branded artwork.
The upstream project does not supply a separate per-brand license inventory.
Keep this notice with redistributed artwork, preserve its upstream licensing,
and consult the respective brand owner for any additional brand-specific terms.

`SOURCE-LICENSE-MIT` covers the upstream association/source code, **not all SVG
artwork**. `mappings.json` and `src/desktop/file_icon_catalog.rs` are generated from
upstream `supportedExtensions.ts`, `supportedFolders.ts`, and `languages.ts` by
`scripts/import-vscode-icons.mjs`. Rodtty includes the enabled default set;
optional upstream presets and project auto-detection are not enabled. Known
language filename/extension associations are applied before explicit overrides,
matching upstream precedence. Filename matches are case insensitive, and compound
extensions use the longest matching suffix. Unrecognized files and folders use
upstream generic artwork.

Regenerate from the pinned checkout with:

    node scripts/import-vscode-icons.mjs /path/to/vscode-icons
    cargo fmt

Node and the upstream checkout are developer import tools only. All mappings and
SVG data are embedded in Rodtty; display requires no network, service, or runtime
asset directory. The current resolved appearance chooses light variants when
upstream supplies them, otherwise the original colored artwork is reused.
