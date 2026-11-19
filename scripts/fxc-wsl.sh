#!/usr/bin/env bash
# fxc.exe stand-in for cross builds from WSL (`gpui_windows/build.rs` invokes it
# via GPUI_FXC_PATH). Runs fxc-lite.exe, which compiles with Windows' own
# d3dcompiler_47.dll. Stages the exe, .hlsl sources (with #include siblings),
# and output header under %LOCALAPPDATA% so WSL interop can run them cleanly.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exe="${FXC_LITE_EXE:-$here/../target/x86_64-pc-windows-gnu/release/fxc-lite.exe}"
[[ -f "$exe" ]] || { echo "fxc-lite.exe not built: $exe (run scripts/regen-windows-shaders.sh)" >&2; exit 1; }
if [[ -z "${FXC_LITE_STAGE:-}" ]]; then
  local_app_data="$(cd /mnt/c && /mnt/c/Windows/System32/cmd.exe /c 'echo %LOCALAPPDATA%' 2>/dev/null | tr -d '\r')"
  [[ "$local_app_data" == ?:\\* ]] || { echo "fxc-wsl: can't resolve %LOCALAPPDATA% (got '$local_app_data')" >&2; exit 1; }
  FXC_LITE_STAGE="$(wslpath -u "$local_app_data")/gpui-fxc-build"
fi
stage="$FXC_LITE_STAGE"
job="$(mktemp -d "$stage/fxc.XXXXXX" 2>/dev/null || { mkdir -p "$stage"; mktemp -d "$stage/fxc.XXXXXX"; })"
trap 'rm -rf "$job"' EXIT
cmp -s "$exe" "$stage/fxc-lite.exe" || cp "$exe" "$stage/fxc-lite.exe"

args=(); header=""
while (($#)); do
  case "$1" in
    /Fh) header="$2"; args+=("/Fh" "$(wslpath -w "$job")\\out.h"); shift 2 ;;
    /T|/E|/Vn) args+=("$1" "$2"); shift 2 ;;
    *) if [[ ! -f "$1" ]]; then args+=("$1"); shift; continue; fi
       src="$1"; cp "$(dirname "$src")"/*.hlsl "$job/"; args+=("$(wslpath -w "$job")\\$(basename "$src")"); shift ;;
  esac
done
[[ -n "$header" ]] || { echo "fxc-wsl: missing /Fh" >&2; exit 1; }
# WSL interop occasionally fails to start a Windows process; retry before giving up.
for attempt in 1 2 3; do
  if (cd "$stage" && ./fxc-lite.exe "${args[@]}"); then break; fi
  status=$?
  echo "fxc-wsl: fxc-lite.exe exited with $status (attempt $attempt)" >&2
  [[ $attempt == 3 ]] && exit "$status"
  sleep 1
done
cp "$job/out.h" "$header"
