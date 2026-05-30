class CandorAi < Formula
  desc "Lawful Good Rust Agentic Operating System — production-grade agent harness"
  homepage "https://github.com/iknowkungfubar/candor-ai"
  url "https://github.com/iknowkungfubar/candor-ai/archive/refs/tags/v1.0.0.tar.gz"
  sha256 "" # Auto-filled by brew on PR
  license "MIT"
  head "https://github.com/iknowkungfubar/candor-ai.git", branch: "main"

  depends_on "rust" => :build
  depends_on "bubblewrap" => :recommended
  depends_on "whisper-cpp" => :optional

  def install
    system "cargo", "install", *std_cargo_args(path: ".")
  end

  test do
    assert_match "candor #{version}", shell_output("#{bin}/candor --version")
    system "#{bin}/candor", "--health"
  end
end
