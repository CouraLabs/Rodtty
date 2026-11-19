class Rodtty < Formula
  desc "Terminal with a file explorer, editor, and Git diffs"
  homepage "https://github.com/CouraLabs/Rodtty"
  version "@VERSION@"
  license "MIT"

  # macOS installs the app with: brew install --cask couralabs/tap/rodtty
  depends_on :linux

  on_intel do
    url "https://github.com/CouraLabs/Rodtty/releases/download/v#{version}/rodtty-#{version}-linux-x86_64.tar.gz"
    sha256 "@SHA_LINUX_X86_64@"
  end
  on_arm do
    url "https://github.com/CouraLabs/Rodtty/releases/download/v#{version}/rodtty-#{version}-linux-aarch64.tar.gz"
    sha256 "@SHA_LINUX_AARCH64@"
  end

  def install
    # rodtty finds rodtty-shell beside its real path.
    libexec.install "rodtty", "rodtty-shell"
    bin.install_symlink libexec/"rodtty"
    (share/"icons/hicolor/256x256/apps").install "rodtty.png"
    inreplace "rodtty.desktop" do |s|
      s.gsub!(/^Exec=.*/, "Exec=#{opt_libexec}/rodtty")
      s.gsub!(/^Icon=.*/, "Icon=#{opt_share}/icons/hicolor/256x256/apps/rodtty.png")
    end
    (share/"applications").install "rodtty.desktop"
  end

  def caveats
    <<~EOS
      To show Rodtty in your app launcher, link its desktop entry:
        mkdir -p ~/.local/share/applications
        ln -sf #{opt_share}/applications/rodtty.desktop ~/.local/share/applications/
    EOS
  end

  test do
    assert_match "ls", shell_output("#{bin}/rodtty --list-utilities")
  end
end
