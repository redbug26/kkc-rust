class Kkc < Formula
  desc "Dual-panel file manager inspired by Norton Commander, written in Rust"
  homepage "https://github.com/redbug26/kkc-rust"
  version "0.1.18"
  license "MIT"

  on_macos do
    url "https://github.com/redbug26/kkc-rust/releases/download/v0.1.18/kkc-macos-arm64.tar.gz"
    sha256 "85ba71204726b7be7883ce166c2c40e0444c64ac0b3d6fec47533306480b5610"
  end

  on_linux do
    url "https://github.com/redbug26/kkc-rust/releases/download/v0.1.18/kkc-linux-x86_64.tar.gz"
    sha256 "be5348acd6bf749ddf101b8b8e85f85ca651ea8024c0afeb73f3bf1ece0b55b6"
  end

  # Build from source with: brew install --HEAD redbug26/kkc-rust/kkc
  head do
    url "https://github.com/redbug26/kkc-rust.git", branch: "main"
    depends_on "rust" => :build
    depends_on "samba" => :build
  end

  def install
    if build.head?
      system "cargo", "install", *std_cargo_args,
             "--features", "smb",
             "--env", "PKG_CONFIG_PATH=#{Formula["samba"].opt_lib}/pkgconfig"
    else
      bin.install "kkc"
    end
  end

  test do
    assert_predicate bin/"kkc", :exist?
  end
end
