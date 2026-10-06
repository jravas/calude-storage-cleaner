# Homebrew cask for Claude Storage Cleaner.
# Lives in the tap repository jravas/homebrew-tap as Casks/claude-storage-cleaner.rb.
# Update `version` and `sha256` for each release; `brew fetch --cask` prints the hash.
cask "claude-storage-cleaner" do
  arch arm: "aarch64", intel: "x64"

  version "0.1.1"
  sha256 arm:   "d250b6fbe41f77c8881b6d1a6d490138adf0c14305789c034fbd1ab98693d5e9",
         intel: "868d2baa21a5371369cd870bc9fa78e6b4a3515500b899007f8f2cd17658323b"

  url "https://github.com/jravas/calude-storage-cleaner/releases/download/v#{version}/Claude.Storage.Cleaner_#{version}_#{arch}.dmg"
  name "Claude Storage Cleaner"
  desc "See where Claude Code's disk usage goes and clean it up safely"
  homepage "https://github.com/jravas/calude-storage-cleaner"

  depends_on macos: ">= :ventura"

  app "Claude Storage Cleaner.app"

  zap trash: [
    "~/Library/Application Support/digital.prototyp.claude-storage-cleaner",
    "~/Library/Logs/cleaner-app",
  ]
end
