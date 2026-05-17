class Kkc < Formula
  desc "Dual-panel file manager inspired by Norton Commander, written in Rust"
  homepage "https://github.com/redbug26/kkc-rust"
  version "0.1.22"
  license "MIT"

  on_macos do
    url "https://github.com/redbug26/kkc-rust/releases/download/v0.1.22/kkc-macos-arm64.tar.gz"
    sha256 "9adfcdfcf818b4347c59150b4dbea02545d815e9c0cdd23b0cdb6185b5d684a8"
  end

  on_linux do
    url "https://github.com/redbug26/kkc-rust/releases/download/v0.1.22/kkc-linux-x86_64.tar.gz"
    sha256 "c641aa3052b84fd23e6fba611c499871c496cfb519cfb1f2a6d433c9aed71a1a"
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
