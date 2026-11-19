#!/usr/bin/env bash
# Recompiles gpui_windows' HLSL shaders and rewrites the committed
# crates/gpui_windows/src/shaders_bytes.rs. Needed only after editing .hlsl
# files. Runs in WSL via Windows' built-in d3dcompiler_47.dll through
# tooling/fxc-lite (see scripts/fxc-wsl.sh).
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
target=x86_64-pc-windows-gnu
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER:-x86_64-w64-mingw32-gcc}"
if [[ -z "${FXC_LITE_STAGE:-}" ]]; then
  lad="$(cd /mnt/c && /mnt/c/Windows/System32/cmd.exe /c 'echo %LOCALAPPDATA%' 2>/dev/null | tr -d '\r')"
  export FXC_LITE_STAGE="$(wslpath -u "$lad")/gpui-fxc-build"
fi
cargo build --release -p fxc-lite --target "$target"
# Touch build.rs so the build script reruns even if nothing else changed.
touch crates/gpui_windows/build.rs
GPUI_REGENERATE_SHADERS=1 GPUI_FXC_PATH="$root/scripts/fxc-wsl.sh" \
  cargo build --release -p gpui_windows --target "$target"
git status --short crates/gpui_windows/src/shaders_bytes.rs
