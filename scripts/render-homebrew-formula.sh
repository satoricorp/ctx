#!/usr/bin/env bash
set -euo pipefail

version=""
linux_sha=""
macos_intel_sha=""
macos_arm_sha=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      version="$2"
      shift 2
      ;;
    --linux-sha)
      linux_sha="$2"
      shift 2
      ;;
    --macos-intel-sha)
      macos_intel_sha="$2"
      shift 2
      ;;
    --macos-arm-sha)
      macos_arm_sha="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$version" || -z "$linux_sha" || -z "$macos_intel_sha" || -z "$macos_arm_sha" ]]; then
  echo "usage: $0 --version <version> --linux-sha <sha> --macos-intel-sha <sha> --macos-arm-sha <sha>" >&2
  exit 1
fi

cat <<EOF
class Ctx < Formula
  desc "Local context for agents and humans"
  homepage "https://github.com/satoricorp/ctx"
  version "${version}"
  license "AGPL-3.0-only"

  if OS.mac? && Hardware::CPU.arm?
    url "https://raw.githubusercontent.com/satoricorp/homebrew-tap/refs/heads/main/dist/ctx-${version}-aarch64-apple-darwin.tar.gz"
    sha256 "${macos_arm_sha}"
  elsif OS.mac?
    url "https://raw.githubusercontent.com/satoricorp/homebrew-tap/refs/heads/main/dist/ctx-${version}-x86_64-apple-darwin.tar.gz"
    sha256 "${macos_intel_sha}"
  else
    url "https://raw.githubusercontent.com/satoricorp/homebrew-tap/refs/heads/main/dist/ctx-${version}-x86_64-unknown-linux-gnu.tar.gz"
    sha256 "${linux_sha}"
  end

  def install
    bin.install "ctx", "ctx-server"
    prefix.install_metafiles
  end

  test do
    system "#{bin}/ctx", "--help"
    system "#{bin}/ctx-server", "--help"
  end
end
EOF
