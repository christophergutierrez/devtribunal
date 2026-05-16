class Devtribunal < Formula
  desc "MCP server where each tool is a specialist code review agent"
  homepage "https://github.com/christophergutierrez/devtribunal"
  version "0.7.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.7.0/devtribunal-aarch64-apple-darwin.tar.gz"
      sha256 "02b76ec8406aebcbb7837f64f1470ddc6faf74cd058ed572019ede66b3ec182b"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.7.0/devtribunal-x86_64-apple-darwin.tar.gz"
      sha256 "35374022f3222ce3f137dbbd31c7c29f63f844442d8ef3b08cad1e5c675eb5b6"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.7.0/devtribunal-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "695f9a2cfc1ad822b267ec1ad5e8b1d4a5d4ed0a345afc69961aef6dc50a5534"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.7.0/devtribunal-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "5e433865b593fc6ee48c1894bd3fa86b8f79c5866ffb75afb1bc38b96f0e71ee"
    end
  end

  def install
    bin.install "devtribunal"
  end

  test do
    system bin/"devtribunal", "--version"
  end
end
