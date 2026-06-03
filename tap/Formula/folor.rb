class Folor < Formula
  desc "Recursive tail -f with glob pattern matching"
  homepage "https://github.com/sdkks/folor"
  url "https://github.com/sdkks/folor/releases/download/vREPLACE_WITH_VERSION/folor-vREPLACE_WITH_VERSION-aarch64-apple-darwin.tar.gz"
  version "REPLACE_WITH_VERSION"
  sha256 "REPLACE_WITH_SHA256"

  def install
    bin.install "folor"
  end

  def caveats
    <<~EOS
      If macOS prevents this binary from running due to Gatekeeper, clear the quarantine flag manually:
        xattr -dr com.apple.quarantine #{opt_bin}/folor
    EOS
  end

  test do
    assert_match "REPLACE_WITH_VERSION", shell_output("#{bin}/folor --version")
  end
end
