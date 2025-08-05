class Wassette < Formula
  desc "Wassette: A security-oriented runtime that runs WebAssembly Components via MCP"
  homepage "https://github.com/microsoft/wassette"
  # Change this to install a different version of wassette.
  # The release tag in GitHub must exist with a 'v' prefix (e.g., v0.1.0).
  version "0.2.0"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/microsoft/wassette/releases/download/v#{version}/wassette_#{version}_darwin_amd64.tar.gz"
      sha256 "4b2624fa6060be45b5b7ab97d45bbe387961653e33edb9fd1fa1bd7678172ad7"
    else
      url "https://github.com/microsoft/wassette/releases/download/v#{version}/wassette_#{version}_darwin_arm64.tar.gz"
      sha256 "67aaaea5be1ed8d56be2a11de5e6eb1e66336fcb7c6fdf3f0df82f9ba6b62642"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/microsoft/wassette/releases/download/v#{version}/wassette_#{version}_linux_amd64.tar.gz"
      sha256 "4d92aba7d31bda7b0dad399f7885a452473440ac20e9d1f764a4e199b4bb387b"
    else
      url "https://github.com/microsoft/wassette/releases/download/v#{version}/wassette_#{version}_linux_arm64.tar.gz"
      sha256 "556560a34d208f21ae6b733a26a3d5d1898cb4aea0989abd4762b38bea717e9b"
    end
  end

  def install
    bin.install "wassette"
  end

  test do
    # Check if the installed binary's version matches the formula's version
    assert_match "wassette-mcp-server #{version}", shell_output("#{bin}/wassette --version")
  end
end
