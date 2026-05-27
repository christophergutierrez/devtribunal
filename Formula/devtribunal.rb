class Devtribunal < Formula
  desc "MCP server where each tool is a specialist code review agent"
  homepage "https://github.com/christophergutierrez/devtribunal"
  version "0.9.0"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.0/devtribunal-aarch64-apple-darwin.tar.gz"
      sha256 "f5164a59f7ca3b8a21ddc144f1e858e93aa450062b4472e6078df01ea0ca3a2f"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.0/devtribunal-x86_64-apple-darwin.tar.gz"
      sha256 "158a70800494a04b969429effd5c745520af506e29dad282cd69a11e7fb97e57"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.0/devtribunal-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "b39d87cf0acf93a789008e30abaa15f5e8da629e80ef85ed2fa24f6faed1cab3"
    elsif Hardware::CPU.intel?
      url "https://github.com/christophergutierrez/devtribunal/releases/download/v0.9.0/devtribunal-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "b794fda07f8f69b53289a2fb19debf2849cf029872c1153ea3ed9c636703bdf3"
    end
  end

  def install
    bin.install "devtribunal"
  end

  test do
    system bin/"devtribunal", "--version"
  end
end
