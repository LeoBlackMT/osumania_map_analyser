# 构建说明 / Build guide

> 本文件是壳（desktop shell）的构建手册。签名 seed 默认从仓库内相对路径读取，**不需要环境变量**。

## 0. 一次性准备

1. 生成签名密钥（只需一次）：

   ```powershell
   cd backend
   go run ./cmd/feedback-keygen -key-id release-2026-10
   ```

2. 把 seed 写进仓库根的 `.secrets/feedback-seed.txt`（**已被 .gitignore 忽略**），把公钥写进 `.secrets/feedback-public.txt`：

   ```
   key_id=release-2026-10
   public_key=<64 hex>
   FEEDBACK_PUBLIC_KEYS=release-2026-10:<64 hex>
   ```

   服务端 `.env` 用 `FEEDBACK_PUBLIC_KEYS`；GitHub Actions 用 repo secret `MMA_FEEDBACK_SIGNING_SEED` 与 variable `MMA_FEEDBACK_KEY_ID`。

## 1. Windows（本机）

```powershell
.\release.ps1                 # Windows + Linux（Linux 走 Docker）
.\release.ps1 -SkipLinux      # 只出 Windows
.\release.ps1 -Linux          # 只出 Linux（等价于只跑 Docker 那一步）
.\release.ps1 -SigningCheckOnly   # 只检查 seed/公钥，不编译
.\release.ps1 -AllowDevSigning    # 本地测试构建（dev seed，服务端需 FEEDBACK_ALLOW_DEV_KEY=true）
```

产物在 `release/<timestamp>-<version>/`。

## 2. Linux（在 Windows 上用 Docker）

`release.ps1` 会自动：

1. 首次构建镜像 `mma-shell-linux-builder`（`desktop/docker/Dockerfile` = `rust:1-bookworm` + Tauri 系统库）；
2. 运行容器：挂载仓库到 `/src`、`-w /src/desktop`、`CARGO_TARGET_DIR=/src/desktop/target-linux`；
3. 容器内执行 `bash build-linux.sh`，tar.gz 落到 `release/`。

seed 通过 `-e` 在运行时注入，**不写进镜像**。需要 Docker Desktop 正在运行。

## 3. Linux（原生，无 Docker）

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev \
  libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev
export MMA_FEEDBACK_SIGNING_SEED="$(cat .secrets/feedback-seed.txt)"
export MMA_FEEDBACK_KEY_ID="release-2026-10"
desktop/build-linux.sh
```

## 4. GitHub Actions

`.github/workflows/shell-build.yml` 在编译前强制要求 repo secret `MMA_FEEDBACK_SIGNING_SEED`；缺失直接失败（防止误发 dev 签名包）。公钥不需要进仓库，只配到服务端。

## 5. 验证

```powershell
# seed 与公钥是否配对（输出应与 .secrets\feedback-public.txt 一致）
cd backend
go run ./cmd/feedback-keygen -derive -public-only

# 离线验签一条信封
go run ./cmd/feedback-keygen -verify-envelope envelope.json -public-key <公钥>
```

---

# English

## One-time setup

Generate a key pair (`go run ./cmd/feedback-keygen -key-id release-2026-10`), then store the seed in `.secrets/feedback-seed.txt` and the public key in `.secrets/feedback-public.txt` (both gitignored). Server `.env` uses `FEEDBACK_PUBLIC_KEYS`; CI uses the `MMA_FEEDBACK_SIGNING_SEED` repository secret and the `MMA_FEEDBACK_KEY_ID` variable.

## Commands

- `.\release.ps1` — Windows + Linux (Linux via Docker).
- `.\release.ps1 -SkipLinux` — Windows only.
- `.\release.ps1 -Linux` — Linux only.
- `.\release.ps1 -SigningCheckOnly` — validate the seed/public key without building.
- Standalone Linux: `desktop/build-linux.sh` after installing the Tauri dependencies and exporting the seed.

Docker image: `desktop/docker/Dockerfile` (`rust:1-bookworm` + Tauri libs). The seed is injected at run time with `-e` and never baked into the image.
