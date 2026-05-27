class Devtribunal < Formula
  desc "MCP server where each tool is a specialist code review agent"
  homepage "https://github.com/christophergutierrez/devtribunal"
  version "0.8.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.8.0/devtribunal-aarch64-apple-darwin.tar.gz"
      sha256 "34f11b87e824c070e4e39c029691fcb688c60d15540c52acf0425f31513384ec"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.8.0/devtribunal-x86_64-apple-darwin.tar.gz"
      sha256 "2f469e68599ea9f14ea2ac9dc64762e1d12cfc93f4cfb2e2ccdef85befde9a64"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.8.0/devtribunal-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "07c2f937bd5bcbda067cf9c0732be77d194a12f31054be0c7b33b88263a2b95f"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.8.0/devtribunal-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "8de429d5ccd42d5d1e1d0d89e553815ad7f7df5d93cda52a8c6a6c49613098d4"
    end
  end

  def install
    bin.install "devtribunal"
  end

  test do
    system bin/"devtribunal", "--version"
  end
end
