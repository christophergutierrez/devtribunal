class Devtribunal < Formula
  desc "MCP server where each tool is a specialist code review agent"
  homepage "https://github.com/christophergutierrez/devtribunal"
  version "0.6.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.6.0/devtribunal-aarch64-apple-darwin.tar.gz"
      sha256 "ff4adc81edf722fc1576d621a2af85cb48a10a6a79d21d0b9da01156268ab49a"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.6.0/devtribunal-x86_64-apple-darwin.tar.gz"
      sha256 "a0c23b6befeb3946d04631fcd261864bc5c40729c69cd72fd5a3eba3c26b271f"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.6.0/devtribunal-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "9c6a73361b41e27aee1fe8e36c425f55b9cc552bba3e321ac2c60e6059724594"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.6.0/devtribunal-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "221b884cf4f5463760fc79e184bb6e59c3735c87fdb6334a9b5bef48d7e70613"
    end
  end

  def install
    bin.install "devtribunal"
  end

  test do
    system bin/"devtribunal", "--version"
  end
end
