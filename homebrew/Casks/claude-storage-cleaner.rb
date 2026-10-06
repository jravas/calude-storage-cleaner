# Homebrew cask for Claude Storage Cleaner.
# Lives in the tap repository jravas/homebrew-tap as Casks/claude-storage-cleaner.rb.
# Update `version` and `sha256` for each release; `brew fetch --cask` prints the hash.
cask "claude-storage-cleaner" do
  arch arm: "aarch64", intel: "x64"

  version "0.1.0"
  sha256 arm:   "f3220935baf4ef83586dbab4f8d96a4ade5a7d434b41dd793a4f6f3a20834793",
         intel: "eec40aab8f257baae59db13bc4385995477f9620523692685234f8b8a7b9a604"

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
