#!/usr/bin/env bash
set -euo pipefail

repo_dir=""
deb_path=""
keyring_out=""
signing_key=""
distribution="stable"
component="main"
architecture="amd64"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo-dir)
      repo_dir="$2"
      shift 2
      ;;
    --deb)
      deb_path="$2"
      shift 2
      ;;
    --keyring-out)
      keyring_out="$2"
      shift 2
      ;;
    --signing-key)
      signing_key="$2"
      shift 2
      ;;
    --distribution)
      distribution="$2"
      shift 2
      ;;
    --component)
      component="$2"
      shift 2
      ;;
    --architecture)
      architecture="$2"
      shift 2
      ;;
    *)
      echo "unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$repo_dir" || -z "$deb_path" || -z "$keyring_out" || -z "$signing_key" ]]; then
  echo "usage: $0 --repo-dir <dir> --deb <file.deb> --keyring-out <file.gpg> --signing-key <key-id>" >&2
  exit 1
fi

if [[ -z "${GNUPGHOME:-}" ]]; then
  echo "GNUPGHOME must be set" >&2
  exit 1
fi

pool_dir="${repo_dir}/pool/${component}/c/ctx"
dist_dir="${repo_dir}/dists/${distribution}"
binary_dir="${dist_dir}/${component}/binary-${architecture}"

mkdir -p "$pool_dir" "$binary_dir"
cp "$deb_path" "$pool_dir/"

(
  cd "$repo_dir"
  dpkg-scanpackages --multiversion "pool/${component}" > "dists/${distribution}/${component}/binary-${architecture}/Packages"
  gzip -fk "dists/${distribution}/${component}/binary-${architecture}/Packages"

  apt-ftparchive \
    -o APT::FTPArchive::Release::Origin="Satori Engineering Co." \
    -o APT::FTPArchive::Release::Label="ctx" \
    -o APT::FTPArchive::Release::Suite="${distribution}" \
    -o APT::FTPArchive::Release::Codename="${distribution}" \
    -o APT::FTPArchive::Release::Architectures="${architecture}" \
    -o APT::FTPArchive::Release::Components="${component}" \
    release "dists/${distribution}" > "dists/${distribution}/Release"
)

gpg --batch --yes --output "$keyring_out" --export "$signing_key"

gpg_args=(
  --batch
  --yes
  --pinentry-mode
  loopback
  --default-key
  "$signing_key"
)

if [[ -n "${APT_GPG_PASSPHRASE:-}" ]]; then
  gpg_args+=(--passphrase "$APT_GPG_PASSPHRASE")
fi

gpg "${gpg_args[@]}" \
  --clearsign \
  --output "${dist_dir}/InRelease" \
  "${dist_dir}/Release"

gpg "${gpg_args[@]}" \
  --detach-sign \
  --armor \
  --output "${dist_dir}/Release.gpg" \
  "${dist_dir}/Release"
