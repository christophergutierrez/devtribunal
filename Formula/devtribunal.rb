class Devtribunal < Formula
  desc "MCP server where each tool is a specialist code review agent"
  homepage "https://github.com/christophergutierrez/devtribunal"
  version "0.9.2"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.2/devtribunal-aarch64-apple-darwin.tar.gz"
      sha256 "d92d92a2bdde8846937017f2a4581778a5ff9c34acba9b2b1bcd659a85debd7f"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.2/devtribunal-x86_64-apple-darwin.tar.gz"
      sha256 "6a439b446455d9fc4f2fb0f843b47ff3ee9feed2b8a79da37dd8437ec6b6355e"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.2/devtribunal-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "bb243a1d8a35271fef68d6b3ba6c295b4e661ad1a1630e9e7a99bf8d95823472"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.2/devtribunal-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "3c93e16cd92d0f94ffbbd05de06b8b9f8d8fbcc36ced48374fe931cf7bfe2702"
    end
  end

  def install
    bin.install "devtribunal"
  end

  test do
    system bin/"devtribunal", "--version"
  end
end
