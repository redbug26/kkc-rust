#!/usr/bin/env python3
"""Regenerate Formula/kkc.rb with the values passed as environment variables.

Required env vars:
  VERSION   - release version, e.g. 0.2.0
  SHA_ARM64 - SHA256 of kkc-macos-arm64.tar.gz
  SHA_X86_64 - SHA256 of kkc-macos-x86_64.tar.gz
  SHA_LINUX - SHA256 of kkc-linux-x86_64.tar.gz
"""
import os
import pathlib

v   = os.environ["VERSION"]
a64 = os.environ["SHA_ARM64"]
x64 = os.environ["SHA_X86_64"]
lin = os.environ["SHA_LINUX"]

formula = (
    'class Kkc < Formula\n'
    '  desc "Dual-panel file manager inspired by Norton Commander, written in Rust"\n'
    '  homepage "https://github.com/redbug26/kkc-rust"\n'
    f'  version "{v}"\n'
    '  license "MIT"\n'
    '\n'
    '  on_macos do\n'
    '    if Hardware::CPU.arm?\n'
    f'      url "https://github.com/redbug26/kkc-rust/releases/download/v{v}/kkc-macos-arm64.tar.gz"\n'
    f'      sha256 "{a64}"\n'
    '    else\n'
    f'      url "https://github.com/redbug26/kkc-rust/releases/download/v{v}/kkc-macos-x86_64.tar.gz"\n'
    f'      sha256 "{x64}"\n'
    '    end\n'
    '  end\n'
    '\n'
    '  on_linux do\n'
    f'    url "https://github.com/redbug26/kkc-rust/releases/download/v{v}/kkc-linux-x86_64.tar.gz"\n'
    f'    sha256 "{lin}"\n'
    '  end\n'
    '\n'
    '  # Build from source with: brew install --HEAD redbug26/kkc-rust/kkc\n'
    '  head do\n'
    '    url "https://github.com/redbug26/kkc-rust.git", branch: "main"\n'
    '    depends_on "rust" => :build\n'
    '  end\n'
    '\n'
    '  def install\n'
    '    if build.head?\n'
    '      system "cargo", "install", *std_cargo_args\n'
    '    else\n'
    '      bin.install "kkc"\n'
    '    end\n'
    '  end\n'
    '\n'
    '  test do\n'
    '    assert_predicate bin/"kkc", :exist?\n'
    '  end\n'
    'end\n'
)

out = pathlib.Path("Formula/kkc.rb")
out.write_text(formula)
print(f"Written {out} for version {v}")
