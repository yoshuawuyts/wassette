class Wassette < Formula
  desc "Wassette: A security-oriented runtime that runs WebAssembly Components via MCP"
  homepage "https://github.com/microsoft/wassette"
  # Change this to install a different version of wassette.
  # The release tag in GitHub must exist with a 'v' prefix (e.g., v0.1.0).
  version "0.2.0"

  on_macos do
    if Hardware::CPU.intel?
      url "https://github.com/microsoft/wassette/releases/download/v#{version}/wassette_#{version}_darwin_amd64.tar.gz"
      sha256 "29484bc445907ced569b22adf1c2ec8a244c55818a3a7e26ed49af7ba1203be2"
    else
      url "https://github.com/microsoft/wassette/releases/download/v#{version}/wassette_#{version}_darwin_arm64.tar.gz"
      sha256 "9ce7611910b430f5f4a631d420743ecc831883178e72309ddc21269efcf1a2c6"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/microsoft/wassette/releases/download/v#{version}/wassette_#{version}_linux_amd64.tar.gz"
      sha256 "ee75388f649244a6fb1d15b864a5551e761213315d1bd4cf710b29b17e4f5435"
    else
      url "https://github.com/microsoft/wassette/releases/download/v#{version}/wassette_#{version}_linux_arm64.tar.gz"
      sha256 "f0461a51b13b24acda002b42df08846d6d001d021d3fd91ea316521d4f417ecf"
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
