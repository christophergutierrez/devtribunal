class Devtribunal < Formula
  desc "MCP server where each tool is a specialist code review agent"
  homepage "https://github.com/christophergutierrez/devtribunal"
  version "0.7.1"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.7.1/devtribunal-aarch64-apple-darwin.tar.gz"
      sha256 "36c16e368bf7662ca3dac3ddf3c75da0006be822096bda0238e63e1412ef5f60"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.7.1/devtribunal-x86_64-apple-darwin.tar.gz"
      sha256 "28ff3a25006cbd4a937efb49f5fd800fb557bc7526353b357226c72d640c062e"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.7.1/devtribunal-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "b0f75f7b6621d81f760da9f20c5d47dfa5da9b49fe40dc3b5e0b59cfef24b78f"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.7.1/devtribunal-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "90bf7968e1d31bd458feed1e85c6b58bfdac9575920befcedbabf597ffc69cf9"
    end
  end

  def install
    bin.install "devtribunal"
  end

  test do
    system bin/"devtribunal", "--version"
  end
end
