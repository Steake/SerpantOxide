#!/usr/bin/env bash
set -euo pipefail

APP_NAME="Serpantoxide"
BUNDLE_ID="com.pentestagent.serpantoxide"
PROFILE="release"
TARGET_TRIPLE=""
OUT_DIR="dist"
ZIP_BUNDLE="0"
SKIP_BUILD="0"
APP_VERSION=""

usage() {
  cat <<'EOF'
Usage: scripts/package_macos_app.sh [options]

Build and assemble a self-contained macOS .app bundle for Serpantoxide.

Options:
  --debug              Build the debug profile instead of release
  --release            Build the release profile (default)
  --target <triple>    Cargo target triple, e.g. aarch64-apple-darwin
  --out-dir <path>     Output directory for the .app bundle (default: dist)
  --bundle-id <id>     CFBundleIdentifier value
  --version <value>    CFBundleShortVersionString / CFBundleVersion value
  --zip                Also create a .zip archive next to the .app bundle
  --skip-build         Reuse an existing binary in target/
  -h, --help           Show this help text
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --debug)
      PROFILE="debug"
      shift
      ;;
    --release)
      PROFILE="release"
      shift
      ;;
    --target)
      TARGET_TRIPLE="${2:?missing target triple}"
      shift 2
      ;;
    --out-dir)
      OUT_DIR="${2:?missing output directory}"
      shift 2
      ;;
    --bundle-id)
      BUNDLE_ID="${2:?missing bundle id}"
      shift 2
      ;;
    --version)
      APP_VERSION="${2:?missing version}"
      shift 2
      ;;
    --zip)
      ZIP_BUNDLE="1"
      shift
      ;;
    --skip-build)
      SKIP_BUILD="1"
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${PROJECT_ROOT}"

if [[ -z "${APP_VERSION}" ]]; then
  APP_VERSION="$(
    python3 - <<'PY'
import pathlib
import tomllib

package = tomllib.loads(pathlib.Path("Cargo.toml").read_text())["package"]
print(package["version"])
PY
  )"
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This packaging script must run on macOS." >&2
  exit 1
fi

if [[ -n "${TARGET_TRIPLE}" ]]; then
  TARGET_ARGS=(--target "${TARGET_TRIPLE}")
  BINARY_PATH="${PROJECT_ROOT}/target/${TARGET_TRIPLE}/${PROFILE}/${APP_NAME}"
else
  TARGET_ARGS=()
  BINARY_PATH="${PROJECT_ROOT}/target/${PROFILE}/${APP_NAME}"
fi

if [[ "${SKIP_BUILD}" != "1" ]]; then
  cargo build --locked "--${PROFILE}" "${TARGET_ARGS[@]}"
fi

if [[ ! -x "${BINARY_PATH}" ]]; then
  echo "Expected built binary at ${BINARY_PATH}" >&2
  exit 1
fi

BUNDLE_DIR="${PROJECT_ROOT}/${OUT_DIR}/${APP_NAME}.app"
CONTENTS_DIR="${BUNDLE_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"
RUNTIME_DIR="${RESOURCES_DIR}/runtime"
LAUNCHER_PATH="${MACOS_DIR}/${APP_NAME}"
REAL_BINARY_PATH="${MACOS_DIR}/${APP_NAME}-bin"
INFO_PLIST_PATH="${CONTENTS_DIR}/Info.plist"
PKGINFO_PATH="${CONTENTS_DIR}/PkgInfo"

rm -rf "${BUNDLE_DIR}"
mkdir -p "${MACOS_DIR}" "${RESOURCES_DIR}" "${RUNTIME_DIR}"
mkdir -p "${RUNTIME_DIR}/loot/artifacts/screenshots" "${RUNTIME_DIR}/loot/images"

cp "${BINARY_PATH}" "${REAL_BINARY_PATH}"
chmod 755 "${REAL_BINARY_PATH}"

cat > "${LAUNCHER_PATH}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

APP_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
RUNTIME_DIR="${APP_ROOT}/Resources/runtime"

export SERPANTOXIDE_HOME="${RUNTIME_DIR}"
cd "${RUNTIME_DIR}"

exec "${APP_ROOT}/MacOS/Serpantoxide-bin" --gpui "$@"
EOF
chmod 755 "${LAUNCHER_PATH}"

cat > "${INFO_PLIST_PATH}" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleExecutable</key>
  <string>${APP_NAME}</string>
  <key>CFBundleIdentifier</key>
  <string>${BUNDLE_ID}</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>${APP_NAME}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${APP_VERSION}</string>
  <key>CFBundleVersion</key>
  <string>${APP_VERSION}</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF

printf 'APPL????' > "${PKGINFO_PATH}"

for file in README.md ARCHITECTURE.md .serpantoxide_config; do
  if [[ -f "${PROJECT_ROOT}/${file}" ]]; then
    cp "${PROJECT_ROOT}/${file}" "${RUNTIME_DIR}/"
  fi
done

for dir in docs python_assets; do
  if [[ -d "${PROJECT_ROOT}/${dir}" ]]; then
    cp -R "${PROJECT_ROOT}/${dir}" "${RUNTIME_DIR}/"
  fi
done

if [[ -f "${PROJECT_ROOT}/loot/notes.json" ]]; then
  cp "${PROJECT_ROOT}/loot/notes.json" "${RUNTIME_DIR}/loot/notes.json"
fi

if [[ -d "${PROJECT_ROOT}/loot/images" ]]; then
  rsync -a --exclude '.DS_Store' "${PROJECT_ROOT}/loot/images/" "${RUNTIME_DIR}/loot/images/"
fi

if [[ -d "${PROJECT_ROOT}/loot/artifacts/screenshots" ]]; then
  rsync -a --exclude '.DS_Store' "${PROJECT_ROOT}/loot/artifacts/screenshots/" "${RUNTIME_DIR}/loot/artifacts/screenshots/"
fi

if [[ -f "${PROJECT_ROOT}/../assets/pentestagent-logo.png" ]]; then
  cp "${PROJECT_ROOT}/../assets/pentestagent-logo.png" "${RESOURCES_DIR}/pentestagent-logo.png"
fi

echo "Created app bundle:"
echo "  ${BUNDLE_DIR}"

if [[ "${ZIP_BUNDLE}" == "1" ]]; then
  ZIP_PATH="${PROJECT_ROOT}/${OUT_DIR}/${APP_NAME}.zip"
  rm -f "${ZIP_PATH}"
  ditto -c -k --keepParent "${BUNDLE_DIR}" "${ZIP_PATH}"
  echo "Created archive:"
  echo "  ${ZIP_PATH}"
fi
