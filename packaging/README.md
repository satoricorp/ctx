# Packaging

`ctx` should split packaging responsibilities in two:

- this repo builds versioned release assets
- package-manager repositories consume those release assets

That keeps the source of truth for binaries here, while still matching how package managers expect to ingest packages.

## What Lives In This Repo

- release tags such as `v0.1.0`
- GitHub Releases with binary archives for macOS and Linux
- a generated Homebrew formula asset (`ctx.rb`)
- a generated Debian package asset (`ctx_<version>_amd64.deb`)

The release workflow is [`.github/workflows/release.yml`](../.github/workflows/release.yml).

## What Should Live Elsewhere

### Homebrew

Keep the actual tap in a separate repository such as:

- `satoricorp/homebrew-tap`

That repository should contain:

- `Formula/ctx.rb`

For each tagged release:

1. Wait for this repo's release workflow to publish the assets.
2. Download the generated `ctx.rb` asset from the GitHub Release.
3. Copy it into `Formula/ctx.rb` in the tap repo.
4. Commit and push the tap update.

Target install flow:

```bash
brew tap satoricorp/tap
brew install ctx
```

### Apt

Do not use this source repo itself as an apt repository.

Instead, publish the generated `.deb` asset into a separate signed apt repository, for example using:

- `reprepro`
- `aptly`

That repo is responsible for:

- `pool/` package storage
- `dists/` metadata
- signed `InRelease` / `Release` files

Target install flow:

```bash
curl -fsSL <repo-key-url> | sudo gpg --dearmor -o /usr/share/keyrings/ctx.gpg
echo "deb [signed-by=/usr/share/keyrings/ctx.gpg] https://<apt-repo> stable main" | sudo tee /etc/apt/sources.list.d/ctx.list
sudo apt update
sudo apt install ctx
```

## Release Flow

1. Bump the version in `Cargo.toml`.
2. Commit it.
3. Tag the release:

```bash
git tag v0.1.0
git push origin v0.1.0
```

4. GitHub Actions publishes:
   - `ctx-<version>-x86_64-unknown-linux-gnu.tar.gz`
   - `ctx-<version>-x86_64-apple-darwin.tar.gz`
   - `ctx-<version>-aarch64-apple-darwin.tar.gz`
   - checksum files for each archive
   - `ctx_<version>_amd64.deb`
   - `ctx_<version>_amd64.deb.sha256`
   - `ctx.rb`

## Why This Split

The release artifacts are derived directly from the source tree, so they belong here.

The package-manager indices do not:

- Homebrew expects a tap repository that contains formula files.
- apt expects a signed repository layout and metadata.

So the clean split is:

- source + releases here
- tap/repository metadata elsewhere
