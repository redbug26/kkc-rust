class Kkc < Formula
  desc "Dual-panel file manager inspired by Norton Commander, written in Rust"
  homepage "https://github.com/redbug26/kkc-rust"
  version "0.1.16"
  license "MIT"

  on_macos do
    url "https://github.com/redbug26/kkc-rust/releases/download/v0.1.16/kkc-macos-arm64.tar.gz"
    sha256 "181bc0a07a40a3c508dd32ed15a085f27dc465408f98fb5711d6324fa23d6d0a"
  end

  on_linux do
    url "https://github.com/redbug26/kkc-rust/releases/download/v0.1.16/kkc-linux-x86_64.tar.gz"
    sha256 "017e1051c14fc1445bd1f1ea874dba10893fe7a857b205fabaad6e736e74ce53"
  end

  # Build from source with: brew install --HEAD redbug26/kkc-rust/kkc
  head do
    url "https://github.com/redbug26/kkc-rust.git", branch: "main"
    depends_on "rust" => :build
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args
    else
      bin.install "kkc"
    end
  end

  test do
    assert_predicate bin/"kkc", :exist?
  end
end
