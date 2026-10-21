import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { AppError, LaunchProgress, LaunchResult, Snapshot } from "./types";

export function getAppError(error: unknown): AppError {
  if (typeof error === "object" && error !== null && "message" in error) {
    return {
      code: "code" in error ? String(error.code) : "unknown",
      message: String(error.message),
    };
  }
  if (typeof error === "string") {
    try {
      return getAppError(JSON.parse(error));
    } catch {
      return { code: "unknown", message: error };
    }
  }
  return { code: "unknown", message: "操作未完成，请重试或导出诊断信息。" };
}

export interface AppApi {
  preview: boolean;
  snapshot: () => Promise<Snapshot>;
  selectInstallation: (platform: string) => Promise<Snapshot | null>;
  saveTarget: (target: number) => Promise<void>;
  launch: (target: number) => Promise<LaunchResult>;
  onProgress: (
    handler: (progress: LaunchProgress) => void,
  ) => Promise<() => void>;
  exportDiagnostics: () => Promise<string | null>;
}

const nativeApi: AppApi = {
  preview: false,
  snapshot: () => invoke<Snapshot>("get_snapshot"),
  async selectInstallation(platform) {
    const mac = platform === "macos" || platform === "darwin";
    const path = await open(
      mac
        ? {
            title: "选择微信应用（WeChat.app 或 Weixin.app）",
            defaultPath: "/Applications",
            filters: [{ name: "macOS 应用", extensions: ["app"] }],
            multiple: false,
            canCreateDirectories: false,
          }
        : {
            title: "选择微信程序",
            filters: [{ name: "微信程序", extensions: ["exe"] }],
            multiple: false,
          },
    );
    if (!path || Array.isArray(path)) return null;
    return invoke<Snapshot>("select_installation", { path });
  },
  saveTarget: (target) => invoke<void>("save_target", { target }),
  launch: (target) => invoke<LaunchResult>("launch", { target }),
  async onProgress(handler) {
    return listen<LaunchProgress>("launch-progress", (event) =>
      handler(event.payload),
    );
  },
  async exportDiagnostics() {
    const path = await save({
      title: "导出诊断信息",
      defaultPath: `more-wechat-diagnostics-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: "诊断文件", extensions: ["json"] }],
    });
    return path ? invoke<string>("export_diagnostics", { path }) : null;
  },
};

const previewSnapshot: Snapshot = {
  platform: "windows",
  osVersion: "Windows 11",
  installation: {
    path: "C:\\Program Files\\Tencent\\Weixin\\Weixin.exe",
    executable: "Weixin.exe",
    version: "4.x（示例）",
  },
  compatibility: {
    status: "unverified",
    message: "当前版本尚未完成实机多开验证，可先启动一个实例。",
    canMultiLaunch: false,
  },
  instances: [],
  clones: [],
  settings: { installPath: null, targetCount: 2 },
  notes: ["界面预览中的安装信息与运行状态均为演示数据。"],
};
let demoProgress: ((progress: LaunchProgress) => void) | null = null;
const previewApi: AppApi = {
  preview: true,
  snapshot: async () => structuredClone(previewSnapshot),
  selectInstallation: async () => structuredClone(previewSnapshot),
  saveTarget: async (target) => {
    previewSnapshot.settings.targetCount = target;
  },
  async launch(target) {
    demoProgress?.({
      stage: "launching",
      message: "正在演示启动流程…",
      current: 0,
      target,
    });
    await new Promise((resolve) => setTimeout(resolve, 700));
    const before = previewSnapshot.instances.length;
    previewSnapshot.instances = [
      {
        pid: 10001,
        executable: previewSnapshot.installation!.path,
        slot: null,
        startedAt: Date.now(),
      },
    ];
    return {
      status: before ? "unchanged" : "success",
      before,
      after: 1,
      target,
      message: "演示完成：真实微信未启动。",
      errors: [],
    };
  },
  onProgress: async (handler) => {
    demoProgress = handler;
    return () => {
      demoProgress = null;
    };
  },
  exportDiagnostics: async () => "界面预览：未写入诊断文件",
};

// Browser preview is an explicit environment. Native failures always remain native errors.
export const appApi: AppApi = isTauri() ? nativeApi : previewApi;
