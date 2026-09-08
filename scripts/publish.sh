#!/usr/bin/env bash
set -euo pipefail

# Aaron workspace release script
# Publishes workspace packages to crates.io in strict topological order.
# Idempotent: Skips any crate version that is already uploaded to crates.io.

CRATES_IN_ORDER=(
  "aaron-build"
  "aaron-core"
  "aaron-tracing"
  "aaron-membership"
  "aaron-control-plane"
  "aaron-shard"
  "aaron-admin"
  "aaron"
)

DRY_RUN=false
SKIP_TESTS=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --skip-tests)
      SKIP_TESTS=true
      shift
      ;;
    --token)
      export CARGO_REGISTRY_TOKEN="$2"
      shift 2
      ;;
    -h|--help)
      echo "Usage: $0 [--dry-run] [--skip-tests] [--token <token>]"
      echo ""
      echo "Options:"
      echo "  --dry-run      Simulate the publishing process without uploading to crates.io"
      echo "  --skip-tests   Skip running 'cargo check' and 'cargo test'"
      echo "  --token TOKEN  Set the crates.io API token"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [ -z "${CARGO_REGISTRY_TOKEN:-}" ] && [ -n "${CRATES_IO_TOKEN:-}" ]; then
  export CARGO_REGISTRY_TOKEN="$CRATES_IO_TOKEN"
fi

# Ensure required tools are installed
for tool in cargo jq curl; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    echo "Error: Required tool '$tool' is not installed or not in PATH." >&2
    exit 1
  fi
done

echo "==> Resolving workspace metadata..."
METADATA_JSON=$(cargo metadata --no-deps --format-version 1)

# Verify all publishable packages are present in CRATES_IN_ORDER
PUBLISHABLE_PACKAGES=$(echo "$METADATA_JSON" | jq -r '.packages[] | select(.publish == null or (.publish | type == "array" and length > 0)) | .name')

for pkg in $PUBLISHABLE_PACKAGES; do
  FOUND=false
  for ordered_pkg in "${CRATES_IN_ORDER[@]}"; do
    if [ "$pkg" = "$ordered_pkg" ]; then
      FOUND=true
      break
    fi
  done
  if [ "$FOUND" != "true" ]; then
    echo "Error: Package '$pkg' is publishable in workspace but missing from CRATES_IN_ORDER." >&2
    exit 1
  fi
done

if [ "$SKIP_TESTS" = "false" ]; then
  echo "==> Running workspace checks and tests..."
  cargo check --workspace --all-targets
  cargo test --workspace
fi

# Determine which packages need to be published
PACKAGES_TO_PUBLISH=()
declare -A PACKAGE_VERSIONS

echo "==> Checking crates.io publication status..."
for pkg in "${CRATES_IN_ORDER[@]}"; do
  pkg_version=$(echo "$METADATA_JSON" | jq -r --arg pkg "$pkg" '.packages[] | select(.name == $pkg) | .version')
  PACKAGE_VERSIONS["$pkg"]="$pkg_version"

  url="https://crates.io/api/v1/crates/${pkg}/${pkg_version}"
  http_code=$(curl -s -o /dev/null -w "%{http_code}" -H "User-Agent: aaron-release-publisher" "$url" || echo "000")

  if [ "$http_code" = "200" ]; then
    echo "  - $pkg v$pkg_version: already published (skipping)"
  else
    echo "  - $pkg v$pkg_version: not published (will publish)"
    PACKAGES_TO_PUBLISH+=("$pkg")
  fi
done

if [ ${#PACKAGES_TO_PUBLISH[@]} -eq 0 ]; then
  echo "==> All packages are up to date on crates.io. Nothing to publish."
  exit 0
fi

echo "==> Packages requiring publication (${#PACKAGES_TO_PUBLISH[@]}): ${PACKAGES_TO_PUBLISH[*]}"

if [ "$DRY_RUN" = "false" ] && [ -z "${CARGO_REGISTRY_TOKEN:-}" ] && [ ! -f "$HOME/.cargo/credentials.toml" ] && [ ! -f "$HOME/.cargo/credentials" ]; then
  echo "Error: Neither CARGO_REGISTRY_TOKEN nor CRATES_IO_TOKEN is set, and no cargo credentials file was found." >&2
  echo "Please set this secret in your repository settings or run 'cargo login'." >&2
  exit 1
fi

MAX_ATTEMPTS=8
RETRY_DELAY=15

for pkg in "${PACKAGES_TO_PUBLISH[@]}"; do
  version="${PACKAGE_VERSIONS[$pkg]}"
  echo "==> Publishing $pkg v$version..."

  if [ "$DRY_RUN" = "true" ]; then
    echo "  [DRY RUN] Would execute: cargo publish -p $pkg --no-verify"
    continue
  fi

  SUCCESS=false
  for attempt in $(seq 1 $MAX_ATTEMPTS); do
    echo "  Attempt $attempt of $MAX_ATTEMPTS: cargo publish -p $pkg --no-verify"
    if cargo publish -p "$pkg" --no-verify; then
      echo "  Successfully published $pkg v$version"
      SUCCESS=true
      break
    else
      echo "  Publish failed on attempt $attempt."
      if [ $attempt -lt $MAX_ATTEMPTS ]; then
        echo "  Waiting ${RETRY_DELAY}s before retrying (waiting for crates.io index propagation)..."
        sleep $RETRY_DELAY
      fi
    fi
  done

  if [ "$SUCCESS" != "true" ]; then
    echo "Error: Failed to publish $pkg v$version after $MAX_ATTEMPTS attempts." >&2
    exit 1
  fi

  # Allow crates.io sparse index to propagate to avoid race conditions with dependent crates
  echo "  Waiting 15s for registry index propagation..."
  sleep 15
done

echo "==> Publication complete."

# Create git tag for the workspace release
FACADE_VERSION="${PACKAGE_VERSIONS[aaron]}"
TAG_NAME="v${FACADE_VERSION}"

if [ "$DRY_RUN" = "true" ]; then
  echo "==> [DRY RUN] Would create git tag '$TAG_NAME' and GitHub release."
  exit 0
fi

if git rev-parse "$TAG_NAME" >/dev/null 2>&1 || git ls-remote --tags origin "refs/tags/$TAG_NAME" 2>/dev/null | grep -q "$TAG_NAME"; then
  echo "==> Git tag '$TAG_NAME' already exists."
else
  echo "==> Creating git tag '$TAG_NAME'..."
  git tag -a "$TAG_NAME" -m "Release $TAG_NAME"
  if git push origin "$TAG_NAME"; then
    echo "==> Pushed git tag '$TAG_NAME' to origin."
    if command -v gh >/dev/null 2>&1 && [ -n "${GITHUB_TOKEN:-}" ]; then
      echo "==> Creating GitHub release for '$TAG_NAME'..."
      gh release create "$TAG_NAME" --title "Aaron $TAG_NAME" --generate-notes || echo "Warning: Failed to create GitHub release."
    fi
  else
    echo "Warning: Could not push git tag '$TAG_NAME' to origin."
  fi
fi
