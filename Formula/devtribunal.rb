class Devtribunal < Formula
  desc "MCP server where each tool is a specialist code review agent"
  homepage "https://github.com/christophergutierrez/devtribunal"
  version "0.9.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.1/devtribunal-aarch64-apple-darwin.tar.gz"
      sha256 "c66ef479d2f1a585c7c5ef70bd0ceafa4ee73c9ec00f0c1270961daab48c388c"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.1/devtribunal-x86_64-apple-darwin.tar.gz"
      sha256 "9b5d571d3e8c7b1a4ff8b5ff321724b1d0aa64f9255680940f4ed113748902a2"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.1/devtribunal-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "bbed0b86c4375b489d2d4af479b58d6bc788e89beb9d18b873934da26e7c4069"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.1/devtribunal-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "94660c2672390d554851e371fdc37c377b3acb7c321ff871d4ab2336052e5c9b"
    end
  end

  def install
    bin.install "devtribunal"
  end

  test do
    system bin/"devtribunal", "--version"
  end
end
