class Kkc < Formula
  desc "Dual-panel file manager inspired by Norton Commander, written in Rust"
  homepage "https://github.com/redbug26/kkc-rust"
  version "0.1.17"
  license "MIT"

  on_macos do
    url "https://github.com/redbug26/kkc-rust/releases/download/v0.1.17/kkc-macos-arm64.tar.gz"
    sha256 "1620e7587deca534f4846d27fd27c134991d4a644d1d4a6eae51123bcc7bcfef"
  end

  on_linux do
    url "https://github.com/redbug26/kkc-rust/releases/download/v0.1.17/kkc-linux-x86_64.tar.gz"
    sha256 "53efb4c33863697e911b145cee6ccbda770e8db712d0090788707857d9c95f76"
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
