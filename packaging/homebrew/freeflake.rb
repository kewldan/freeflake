# Template for kewldan/homebrew-tap; the release workflow fills it in.
class Freeflake < Formula
  desc "Free, open-source snow for your desktop, with an advent Christmas tree"
  homepage "https://github.com/kewldan/freeflake"
  version "@VERSION@"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/kewldan/freeflake/releases/download/v@VERSION@/freeflake-@VERSION@-aarch64-apple-darwin.tar.gz"
      sha256 "@SHA_MAC_ARM@"
    end
    on_intel do
      url "https://github.com/kewldan/freeflake/releases/download/v@VERSION@/freeflake-@VERSION@-x86_64-apple-darwin.tar.gz"
      sha256 "@SHA_MAC_X86@"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/kewldan/freeflake/releases/download/v@VERSION@/freeflake-@VERSION@-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA_LINUX_ARM@"
    end
    on_intel do
      url "https://github.com/kewldan/freeflake/releases/download/v@VERSION@/freeflake-@VERSION@-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA_LINUX_X86@"
    end
  end

  def install
    bin.install "freeflake"
  end

  service do
    run opt_bin/"freeflake"
    keep_alive false
  end

  test do
    assert_predicate bin/"freeflake", :executable?
  end
end
