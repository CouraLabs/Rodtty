cask "rodtty" do
  version "@VERSION@"
  sha256 "@SHA_MACOS@"

  url "https://github.com/CouraLabs/Rodtty/releases/download/v#{version}/Rodtty-#{version}-macos-universal.zip"
  name "Rodtty"
  desc "Terminal with a file explorer, editor, and Git diffs"
  homepage "https://github.com/CouraLabs/Rodtty"

  depends_on macos: ">= :big_sur"

  app "Rodtty.app"

  zap trash: [
    "~/Library/Application Support/dev.CouraLabs.Rodtty",
    "~/Library/Caches/dev.CouraLabs.Rodtty",
  ]
@CAVEATS@end
