# Release Process

This site is a private Next.js app managed with Bun. Releases are recorded with
the `package.json` version and an annotated git tag.

## Prerequisites

- Start from `main` with a clean worktree.
- Install dependencies with `bun install` if `node_modules` is missing or stale.
- Review the relevant Next.js docs in `node_modules/next/dist/docs/` before
  changing app code; this project uses Next.js 16 APIs.

## Checklist

1. Confirm the working tree is clean:

   ```bash
   git status --short --branch
   ```

2. Run the release checks:

   ```bash
   bun run lint
   bun run build
   ```

3. Bump `version` in `package.json` using semver:

   - Patch: bug fixes and documentation-only releases.
   - Minor: new user-visible functionality.
   - Major: breaking changes.

4. Commit the release:

   ```bash
   git add RELEASE.md package.json
   git commit -m "Release vX.Y.Z"
   ```

5. Create an annotated tag:

   ```bash
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   ```

6. Push the release when a remote is configured:

   ```bash
   git push origin main
   git push origin vX.Y.Z
   ```

## Production Verification

After deployment, verify the production site by checking the homepage, install
command copy flow, theme toggle, and navigation menu on desktop and mobile
viewports.
