#!/bin/bash
# 在本地 docker container 內模擬 GHA release-linux 的 22.04 / 24.04 build 流程
# 用途:在 push 觸發 GHA 前,先驗證 .github/workflows/release.yml 的 22.04+ 分支邏輯
#
# 用法:
#   tests/docker-ubuntu-build.sh 22.04        # 跑 ubuntu:22.04 build
#   tests/docker-ubuntu-build.sh 24.04        # 跑 ubuntu:24.04 build
#   tests/docker-ubuntu-build.sh 22.04 install  # build + install 在另一乾淨容器
#
# 隔離設計:host 專案目錄以 read-only mount 到 /source,container 內 cp -a 到 /workspace
# 後刪掉 node_modules + src-tauri/target(host macOS arm64 殘留會撞 linux 平台 binding)
set -e

DISTRO_VER="${1:-22.04}"
MODE="${2:-build}"

case "$DISTRO_VER" in
  22.04|24.04) ;;
  *) echo "ERR: distro 只支援 22.04 / 24.04(20.04 用 GHA cache 跑,本地太慢)"; exit 1 ;;
esac

IMAGE="ubuntu:${DISTRO_VER}"
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DISTRO="ubuntu-${DISTRO_VER}"
DEB_DEPENDS="libwebkit2gtk-4.1-0, libgtk-3-0, libayatana-appindicator3-1"
mkdir -p "${PROJECT_ROOT}/tests/tarballs"

echo "==== 1/3 build:在 ${IMAGE} container 內跑 Tauri .deb build ===="
docker run --rm \
  -v "${PROJECT_ROOT}:/source:ro" \
  -v "${PROJECT_ROOT}/tests/tarballs:/output:rw" \
  -e DEBIAN_FRONTEND=noninteractive \
  -e TZ=Etc/UTC \
  -e DISTRO="${DISTRO}" \
  -e DEB_DEPENDS="${DEB_DEPENDS}" \
  -e SOURCE_BUILD=false \
  -e TAG_NAME=v0.1.0 \
  "${IMAGE}" \
  bash -c '
    set -e

    echo "----- 複製 source 到 /workspace(隔離 host node_modules / target)-----"
    mkdir -p /workspace
    cp -a /source/. /workspace/
    cd /workspace
    # yarn.lock 刻意保留:它含全部平台的 optional binding(darwin/linux/win32),在 linux 照樣裝得到
    # @tauri-apps/cli-linux-x64-gnu,留著才能驗證「版控的 lock 檔在 Linux 可重現建置」。
    # (這與舊的 package-lock.json 不同 —— 那個會被 host npm 鎖死成 darwin-arm64 而缺其他平台 entry,故仍刪。)
    rm -rf node_modules package-lock.json src-tauri/target

    echo "----- Bootstrap apt -----"
    apt-get update
    apt-get install -y --no-install-recommends \
      curl wget ca-certificates lsb-release tzdata \
      git build-essential pkg-config patchelf file xz-utils unzip \
      libssl-dev libgtk-3-dev libxdo-dev librsvg2-dev \
      libayatana-appindicator3-dev libsoup-3.0-dev \
      libwebkit2gtk-4.1-dev libcups2-dev

    echo "----- 版本檢查 -----"
    gcc --version | head -1
    pkg-config --modversion webkit2gtk-4.1
    pkg-config --modversion libsoup-3.0
    pkg-config --modversion glib-2.0

    echo "----- Setup Rust -----"
    curl --proto =https --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
    export PATH="$HOME/.cargo/bin:$PATH"

    # Node 22:vue-i18n 的 engines 要求 `>= 22`,Node 20 會被 yarn 的 engines 檢查擋下
    echo "----- Setup Node 22 -----"
    curl -fsSL https://deb.nodesource.com/setup_22.x | bash -
    apt-get install -y nodejs

    echo "----- yarn install(linux 平台 binding 會被裝起來)-----"
    npm i -g yarn
    # --frozen-lockfile:同時驗證「版控的 yarn.lock 在 Linux 可原封不動安裝」——
    # 若 lock 缺 linux binding 或與 package.json 不一致,這裡就會 fail 而非默默改寫
    yarn install --frozen-lockfile

    echo "----- tauri build .deb -----"
    npm run tauri:build -- --bundles deb

    echo "----- Rewrite .deb Package + Depends -----"
    DEB_DIR=src-tauri/target/release/bundle/deb
    ORIG_DEB=$(ls "$DEB_DIR"/*.deb | head -1)
    echo "原始 .deb: $ORIG_DEB"
    WORK=/tmp/deb-fix
    rm -rf "$WORK"
    dpkg-deb -R "$ORIG_DEB" "$WORK"
    echo "=== 原 control ==="
    cat "$WORK/DEBIAN/control"
    sed -i "s/^Package:.*/Package: cix3752i-label-print/" "$WORK/DEBIAN/control"
    sed -i "s/^Depends:.*/Depends: ${DEB_DEPENDS}/" "$WORK/DEBIAN/control"
    echo "=== 新 control ==="
    cat "$WORK/DEBIAN/control"
    ARCH=$(dpkg --print-architecture)
    NEW_DEB="$DEB_DIR/cix3752i-label-print_0.1.0_${ARCH}.deb"
    rm -f "$ORIG_DEB"
    dpkg-deb -b "$WORK" "$NEW_DEB"
    ls -lh "$DEB_DIR"

    echo "----- 打 tarball -----"
    VERSION="${TAG_NAME#v}"
    STAGE_NAME="cix3752iLabelPrint-${VERSION}-${DISTRO}"
    STAGE="/tmp/${STAGE_NAME}"
    mkdir -p "$STAGE"
    cp "$NEW_DEB" "$STAGE/"
    cat > "$STAGE/install.sh" <<INSTALL_EOF
#!/bin/bash
set -e
cd "\$(dirname "\$0")"

echo "=== 智配通 面單列印 — ${DISTRO} 安裝 ==="
echo ""
echo "1/2 透過 apt 安裝主程式與系統依賴(webkit2gtk-4.1 / gtk-3 / appindicator3)"
sudo apt-get update
sudo apt-get install -y ./\$(ls *.deb | head -1)

echo "2/2 完成"
INSTALL_EOF
    chmod +x "$STAGE/install.sh"
    cd /tmp
    tar czf "${STAGE_NAME}.tar.gz" "${STAGE_NAME}"
    ls -lh "${STAGE_NAME}.tar.gz"
    cp "/tmp/${STAGE_NAME}.tar.gz" /output/
    echo "tarball 已輸出到 host:tests/tarballs/${STAGE_NAME}.tar.gz"
  '

echo ""
echo "==== build 完成 ===="

if [ "$MODE" = "install" ]; then
  echo ""
  echo "==== 2/3 install:在乾淨 ${IMAGE} container 內試 install.sh ===="
  TARBALL_NAME="cix3752iLabelPrint-0.1.0-${DISTRO}.tar.gz"
  docker run --rm \
    -v "${PROJECT_ROOT}/tests/tarballs:/tarballs:ro" \
    -e DEBIAN_FRONTEND=noninteractive \
    "${IMAGE}" \
    bash -c "
      set -e
      apt-get update && apt-get install -y --no-install-recommends sudo
      cp /tarballs/${TARBALL_NAME} /tmp/
      cd /tmp
      tar xzf ${TARBALL_NAME}
      cd cix3752iLabelPrint-0.1.0-${DISTRO}
      ls -la
      cat install.sh
      bash install.sh
      echo '----- 驗證 cix3752i-label-print 已安裝 -----'
      which cix3752i-label-print && cix3752i-label-print --version 2>/dev/null || true
      ldd \$(which cix3752i-label-print) | head -20
    "
  echo "==== install 測試完成 ===="
fi
