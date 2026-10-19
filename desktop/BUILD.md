# 构建说明 / Build guide

## 中文

### 0. 一次性准备

1. 生成密钥对（只需一次）：

   ```powershell
   cd backend
   go run ./cmd/feedback-keygen -key-id release-2026-10
   ```

2. 把 seed 写进仓库根的 `.secrets/feedback-seed.txt`（已被 `.gitignore` 忽略），把公钥写进 `.secrets/feedback-public.txt`：

   ```
   key_id=release-2026-10
   public_key=<64 hex>
   FEEDBACK_PUBLIC_KEYS=release-2026-10:<64 hex>
   ```

   服务端 `.env` 用 `FEEDBACK_PUBLIC_KEYS`；GitHub Actions 用 repo secret `MMA_FEEDBACK_SIGNING_SEED` 与 variable `MMA_FEEDBACK_KEY_ID`。

### 1. Windows（本机）

```powershell
.\release.ps1                     # Windows + Linux（Linux 走 Docker）
.\release.ps1 -SkipLinux          # 只出 Windows
.\release.ps1 -Linux              # 只出 Linux
.\release.ps1 -SigningCheckOnly   # 只检查 seed/公钥，不编译
.\release.ps1 -AllowDevSigning    # 本地测试构建（dev seed，服务端需 FEEDBACK_ALLOW_DEV_KEY=true）
```

产物在 `release/<timestamp>-<version>/`。

### 2. Linux（在 Windows 上用 Docker）

`release.ps1` 会自动：构建镜像 `mma-shell-linux-builder`（`desktop/docker/Dockerfile`）→ 挂载仓库到 `/src`、`-w /src/desktop`、`CARGO_TARGET_DIR=/src/desktop/target-linux` → 容器内执行 `bash build-linux.sh`。seed 通过 `-e` 在运行时注入，不写进镜像。需要 Docker Desktop 正在运行。

镜像内已把 Debian 源换成阿里云镜像（`deb.debian.org` 在部分网络下 502）。

### 3. Linux（原生，无 Docker）

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
export MMA_FEEDBACK_SIGNING_SEED="$(cat .secrets/feedback-seed.txt)"
export MMA_FEEDBACK_KEY_ID="release-2026-10"
desktop/build-linux.sh
```

### 4. GitHub Actions

`.github/workflows/shell-build.yml` 编译前强制要求 repo secret `MMA_FEEDBACK_SIGNING_SEED`；缺失直接失败。公钥只配服务端，不进仓库。

### 5. 验证

```powershell
cd backend
go run ./cmd/feedback-keygen -derive -public-only    # 应与 .secrets/feedback-public.txt 一致
go run ./cmd/feedback-keygen -verify-envelope envelope.json -public-key <公钥>
```

---

## English

### 0. One-time setup

1. Generate a key pair (once):

   ```powershell
   cd backend
   go run ./cmd/feedback-keygen -key-id release-2026-10
   ```

2. Put the seed into `.secrets/feedback-seed.txt` (gitignored) and the public key into `.secrets/feedback-public.txt`:

   ```
   key_id=release-2026-10
   public_key=<64 hex>
   FEEDBACK_PUBLIC_KEYS=release-2026-10:<64 hex>
   ```

   The server reads `FEEDBACK_PUBLIC_KEYS` from its `.env`; GitHub Actions uses the `MMA_FEEDBACK_SIGNING_SEED` repository secret and the `MMA_FEEDBACK_KEY_ID` variable.

### 1. Windows (local)

```powershell
.\release.ps1                     # Windows + Linux (Linux via Docker)
.\release.ps1 -SkipLinux          # Windows only
.\release.ps1 -Linux              # Linux only
.\release.ps1 -SigningCheckOnly   # validate the key pair without building
.\release.ps1 -AllowDevSigning    # local test build (dev seed; server needs FEEDBACK_ALLOW_DEV_KEY=true)
```

Artifacts land in `release/<timestamp>-<version>/`.

### 2. Linux (via Docker on Windows)

`release.ps1` builds the `mma-shell-linux-builder` image (`desktop/docker/Dockerfile`), mounts the repo at `/src` with `-w /src/desktop` and `CARGO_TARGET_DIR=/src/desktop/target-linux`, then runs `bash build-linux.sh` inside the container. The seed is injected with `-e` at run time and never baked into the image. Docker Desktop must be running.

The image already switches Debian mirrors to Aliyun, because `deb.debian.org` returns 502 on some networks.

### 3. Linux (native, no Docker)

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
export MMA_FEEDBACK_SIGNING_SEED="$(cat .secrets/feedback-seed.txt)"
export MMA_FEEDBACK_KEY_ID="release-2026-10"
desktop/build-linux.sh
```

### 4. GitHub Actions

`.github/workflows/shell-build.yml` requires the `MMA_FEEDBACK_SIGNING_SEED` repository secret before compiling and fails when it is missing. The public key is only configured on the server, never committed.

### 5. Verification

```powershell
cd backend
go run ./cmd/feedback-keygen -derive -public-only    # must match .secrets/feedback-public.txt
go run ./cmd/feedback-keygen -verify-envelope envelope.json -public-key <public key>
```
