# Appearance palette sources

Rodtty offers exactly twelve families and twenty-four appearances. Family and
Light / Dark / System are independent preferences. Defaults remain Catppuccin
and System. System follows live host changes; explicit Light/Dark stays fixed.
The searchable Settings dropdown shows every family without cycling through them.

| Family | Light | Dark | Primary source and notice |
| --- | --- | --- | --- |
| Catppuccin | Latte | Mocha | [palette](https://github.com/catppuccin/palette), [MIT notice](Catppuccin-LICENSE.txt) |
| Dracula | Alucard | Dracula | [canonical palette](https://github.com/dracula/dracula-theme), [MIT notice](dracula-LICENSE.txt), [VS Code roles notice](dracula-vscode-LICENSE.txt) |
| Tokyo Night | Light | Night | [official editor themes](https://github.com/enkia/tokyo-night-vscode-theme), [MIT notice](tokyonight-LICENSE.txt) |
| Nord | Light | Dark | [canonical palette](https://github.com/nordtheme/nord), [MIT notice](nord-LICENSE.txt), [official editor roles notice](nord-vscode-LICENSE.txt) |
| Gruvbox | Light (medium) | Dark (medium) | [Vim palette and roles](https://github.com/morhetz/gruvbox), [MIT/X11 notice](gruvbox-NOTICE.txt) |
| One Dark Pro | **Rodtty light adaptation** | Dark | [official dark theme](https://github.com/Binaryify/OneDark-Pro), [MIT notice](onedarkpro-LICENSE.txt) |
| Night Owl | Light Owl | Night Owl | [official editor themes](https://github.com/sdras/night-owl-vscode-theme), [MIT notice](nightowl-LICENSE.txt) |
| Ayu | Light | Dark | [ayu-colors v7.3.2](https://github.com/ayu-theme/ayu-colors/tree/b0a0ad47792ef7e9e70f272798dcd7fce357bd0c), [MIT notice](ayu-v7.3.2-LICENSE.txt) |
| Solarized | Light | Dark | [Xresources and Vim roles](https://github.com/altercation/solarized), [MIT notice](solarized-LICENSE.txt) |
| Rosé Pine | Dawn | Moon | [palette](https://github.com/rose-pine/palette), [MIT notice](rosepine-NOTICE.txt) |
| Kanagawa | Lotus | Wave | [Neovim theme palettes](https://github.com/rebelot/kanagawa.nvim), [MIT notice](kanagawa-NOTICE.txt) |
| Everforest | Light | Dark Green | [Vim/Neovim palette](https://github.com/sainnhe/everforest), [MIT+ notice](everforest-NOTICE.txt) |

[sources.json](sources.json) records exact verified Git blob SHA values, original
repository/path, retrieved tree IDs and immutable Git blob API URLs. Tree IDs
are not described as commit IDs. Catppuccin palette v1.8.0 uses blob
`2a5d25cb0b00033bb049d11ab12c8bed478bf4e8`. Ayu's literal RGBs are pinned to
commit `b0a0ad47792ef7e9e70f272798dcd7fce357bd0c` (v7.3.2); the newer generated
YAML palette is not evaluated or used.

## Semantic mapping and adaptations

`catppuccin.json` and `families.json` hold the appearance data. All surfaces use
one resolved appearance: chrome, controls, explorer, terminal defaults, sixteen
ANSI slots, text selection/focus, editor syntax and read-only diff roles.
Selection colors retain their upstream alpha, so blending occurs on the actual
destination surface. Surfaces and thin separators are Rodtty mappings. Editor
and diff syntax share a single explicit token palette through the owned
Rune Kit adapter, rather than Rune Kit's default synthetic hues.

The `syntax` values are upstream literal token colors where supplied. Small
text, muted labels and focus accents with contrast below 4.5:1 against the
canvas are moved toward the appearance's ink, preserving the hue as far as
possible. This is a Rodtty readability adaptation; terminal ANSI RGBs remain
exact, including historically low-contrast program colors. Diff line/gutter/
word tints use the family's inserted/deleted colors and the widget's existing
light/dark opacity hierarchy, with readable syntax drawn above them.

Specific mappings are intentionally explicit:

- Dracula Alucard uses canonical background/foreground/selection and accent
  RGBs. Its surfaces and complete ANSI arrangement are Rodtty choices; no
  official sixteen-slot Alucard ANSI table is claimed. Dark Dracula retains
  the official VS Code ANSI table and syntax roles.
- Tokyo Night uses the official Light and Night editor palettes and all
  sixteen terminal slots. The workspace canvas uses the editor background,
  including the terminal; raised panels use sidebar roles.
- Nord Dark uses the official VS Code roles/ANSI. Nord Light maps the canonical
  polar neutrals to light surfaces and deliberately darkens frost/aurora
  accents for readable light text and ANSI. This is a Rodtty semantic mapping,
  not a claim of an official Nord Light editor palette.
- Gruvbox uses **medium** backgrounds `#fbf1c7` / `#282828`. Normal ANSI slots
  use upstream neutral accents; bright slots use faded accents in Light and
  bright accents in Dark. Syntax follows those faded/bright roles.
- One Dark Pro Light is an original Rodtty adaptation: `#fafafa` canvas,
  `#383a42` ink and darker chromatic accents. Its complete visible name always
  says “One Dark Pro Light (Rodtty adaptation)”. Dark uses the official theme.
- Night Owl uses the official Light Owl / Night Owl editor roles and ANSI.
- Ayu uses v7.3.2 literal common/syntax/VCS colors. That source has no complete
  terminal ANSI table; Rodtty deliberately maps syntax markup/string/function/
  entity/constant/tag colors into ANSI slots. Light small text is adjusted for
  contrast; the source accent colors remain recorded unchanged.
- Solarized retains Xresources' unusual exact ANSI ordering, including orange,
  violet and bright neutral slots. Light swaps the neutral base definitions;
  chromatic accents stay constant. Syntax follows the upstream Vim mappings.

Extended indexed output (16–255) and explicit terminal truecolor retain the
program's RGB values. Theme changes do not replace terminal sessions, alter
selection, or change fonts and terminal metrics. Native lifecycle checks cycle
all nine families through Light/Dark/System against the same running session.
