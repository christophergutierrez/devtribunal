class Devtribunal < Formula
  desc "MCP server where each tool is a specialist code review agent"
  homepage "https://github.com/christophergutierrez/devtribunal"
  version "0.5.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.5.0/devtribunal-aarch64-apple-darwin.tar.gz"
      sha256 "a482f348458c0b8f9eddc2fe21382cd9d23d618b485cee76c3dfc9f1fa228933"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.5.0/devtribunal-x86_64-apple-darwin.tar.gz"
      sha256 "16a9eb8416d434e4c22d5a85e7357905510d2c913bce6292726cc9e0e46b892e"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.5.0/devtribunal-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "71c088f25e97835cc33cda5c0fa7fbd5aed24883d700e3b11e4f29e8269fb122"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.5.0/devtribunal-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "1e77a89e3a945c1983948d677bc5cb668b71866f8fab7a7700d27407061ccd8a"
    end
  end

  def install
    bin.install "devtribunal"
  end

  test do
    system bin/"devtribunal", "--version"
  end
end
