export interface Settings {
  installPath: string | null;
  targetCount: number;
}
export interface Installation {
  path: string;
  executable: string;
  version: string;
}
export interface Compatibility {
  status: "verified" | "unverified" | "unsupported";
  message: string;
  canMultiLaunch: boolean;
}
export interface Instance {
  pid: number;
  executable: string;
  slot: number | null;
  startedAt: number;
}
export interface CloneInfo {
  slot: number;
  path: string;
  outdated: boolean;
  running: boolean;
}
export interface Snapshot {
  platform: string;
  osVersion: string;
  installation: Installation | null;
  compatibility: Compatibility;
  instances: Instance[];
  clones: CloneInfo[];
  settings: Settings;
  notes: string[];
}
export interface LaunchProgress {
  stage: string;
  message: string;
  current: number;
  target: number;
}
export interface LaunchResult {
  status: "success" | "partial" | "failed" | "unchanged";
  before: number;
  after: number;
  target: number;
  message: string;
  errors: string[];
}
export interface AppError {
  code: string;
  message: string;
}
