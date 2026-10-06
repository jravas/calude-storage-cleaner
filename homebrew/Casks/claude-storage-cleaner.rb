# Homebrew cask for Claude Storage Cleaner.
# Lives in the tap repository jravas/homebrew-tap as Casks/claude-storage-cleaner.rb.
# Update `version` and `sha256` for each release; `brew fetch --cask` prints the hash.
cask "claude-storage-cleaner" do
  arch arm: "aarch64", intel: "x64"

  version "0.1.0"
  sha256 arm:   "REPLACE_WITH_ARM64_DMG_SHA256",
         intel: "REPLACE_WITH_X64_DMG_SHA256"

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
