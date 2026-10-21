#!/bin/bash
set -euo pipefail
# Compatibility entrypoint; validation lives in the desktop app.
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
for app in "$HOME/Applications/more-WeChat.app" "/Applications/more-WeChat.app" \
  "$script_dir/src-tauri/target/release/bundle/macos/more-WeChat.app"; do
  if [[ -d "$app" ]]; then
    exec /usr/bin/open "$app"
  fi
done
printf '%s\n' '请先安装 more-WeChat 桌面预览版，或在项目目录运行 pnpm tauri dev。' '安装和兼容说明见 readme.md。' >&2
exit 1
