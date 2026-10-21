import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LaunchProgress, LaunchResult, Snapshot } from "./types";

const api = vi.hoisted(() => ({
  preview: false,
  snapshot: vi.fn(),
  selectInstallation: vi.fn(),
  saveTarget: vi.fn(),
  launch: vi.fn(),
  onProgress: vi.fn(),
  exportDiagnostics: vi.fn(),
}));
vi.mock("./api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./api")>();
  return { ...actual, appApi: api };
});
import App from "./App";

function state(overrides: Partial<Snapshot> = {}): Snapshot {
  return {
    platform: "windows",
    osVersion: "Windows 11",
    installation: {
      path: "C:\\微信\\Weixin.exe",
      executable: "Weixin.exe",
      version: "4.1.0",
    },
    compatibility: {
      status: "unverified",
      message: "尚未完成实机验证",
      canMultiLaunch: true,
    },
    instances: [],
    clones: [],
    settings: { installPath: null, targetCount: 2 },
    notes: [],
    ...overrides,
  };
}

async function ready() {
  await screen.findByText("微信 WeChat");
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "重新检测" })).toBeEnabled(),
  );
}
const success: LaunchResult = {
  status: "success",
  before: 0,
  after: 2,
  target: 2,
  message: "两个实例已稳定运行",
  errors: [],
};

beforeEach(() => {
  vi.resetAllMocks();
  api.preview = false;
  api.snapshot.mockResolvedValue(state());
  api.saveTarget.mockResolvedValue(undefined);
  api.onProgress.mockResolvedValue(vi.fn());
  api.launch.mockResolvedValue(success);
});

describe("微信启动界面", () => {
  it("未安装时提供手动选择，禁止启动", async () => {
    api.snapshot.mockResolvedValue(state({ installation: null }));
    render(<App />);
    expect(await screen.findByText("选择你的微信")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "启动至 2 个实例" }),
    ).toBeDisabled();
    expect(
      screen.getByRole("button", { name: "手动选择微信安装位置" }),
    ).toBeEnabled();
  });

  it("部分成功显示真实数量及错误", async () => {
    api.launch.mockResolvedValue({
      ...success,
      status: "partial",
      after: 1,
      message: "只启动了一个实例",
      errors: ["第二个实例在启动后退出"],
    });
    render(<App />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "启动至 2 个实例" }));
    expect(await screen.findByText("部分启动成功")).toBeInTheDocument();
    expect(screen.getByText("当前 1 个 / 目标 2 个")).toBeInTheDocument();
    expect(screen.getByText("第二个实例在启动后退出")).toBeInTheDocument();
  });

  it("权限不足显示原始错误，不回退为演示成功", async () => {
    api.launch.mockRejectedValue({
      code: "permission_denied",
      message: "无法访问此微信进程",
    });
    render(<App />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "启动至 2 个实例" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("权限不足");
    expect(screen.getByRole("alert")).toHaveTextContent("无法访问此微信进程");
    expect(screen.queryByText("微信已启动")).not.toBeInTheDocument();
    expect(
      screen.queryByText("界面预览 · 不会启动微信"),
    ).not.toBeInTheDocument();
  });

  it("启动中禁止重复提交，先订阅进度再调用启动，完成后解除订阅", async () => {
    let complete!: (result: LaunchResult) => void;
    let update!: (progress: LaunchProgress) => void;
    const unlisten = vi.fn();
    api.onProgress.mockImplementation(
      async (handler: (progress: LaunchProgress) => void) => {
        update = handler;
        return unlisten;
      },
    );
    api.launch.mockImplementation(
      () =>
        new Promise<LaunchResult>((resolve) => {
          complete = resolve;
        }),
    );
    render(<App />);
    await ready();
    const button = screen.getByRole("button", { name: "启动至 2 个实例" });
    fireEvent.click(button);
    fireEvent.click(button);
    await waitFor(() => expect(api.launch).toHaveBeenCalledTimes(1));
    expect(api.onProgress.mock.invocationCallOrder[0]).toBeLessThan(
      api.launch.mock.invocationCallOrder[0],
    );
    expect(
      screen.getByRole("button", { name: "正在启动，请稍候…" }),
    ).toBeDisabled();
    act(() =>
      update({
        stage: "verifying",
        message: "正在确认新进程稳定运行…",
        current: 1,
        target: 2,
      }),
    );
    expect(screen.getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "50",
    );
    expect(screen.getByText("正在确认新进程稳定运行…")).toBeInTheDocument();
    await act(async () => complete(success));
    await waitFor(() => expect(unlisten).toHaveBeenCalledOnce());
    expect(screen.getByText("微信已启动")).toBeInTheDocument();
  });

  it("不支持多开时允许切换到单实例，并保存偏好", async () => {
    api.snapshot.mockResolvedValue(
      state({
        compatibility: {
          status: "unverified",
          message: "当前版本的互斥锁尚未确认",
          canMultiLaunch: false,
        },
      }),
    );
    render(<App />);
    await ready();
    expect(
      screen.getByRole("button", { name: "启动至 2 个实例" }),
    ).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "先启动一个微信" }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "启动微信" })).toBeEnabled(),
    );
    expect(api.saveTarget).toHaveBeenCalledWith(1);
    fireEvent.click(screen.getByRole("button", { name: "减少目标数量" }));
    expect(screen.getByRole("button", { name: "减少目标数量" })).toBeDisabled();
  });

  it("未验证但具备适配能力的版本允许尝试多开", async () => {
    render(<App />);
    await ready();
    expect(screen.getByText("此版本的多开能力尚未验证")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "启动至 2 个实例" }),
    ).toBeEnabled();
  });

  it("显示已验收的系统组合，同时保留验证范围说明", async () => {
    api.snapshot.mockResolvedValue(
      state({
        compatibility: {
          status: "verified",
          message: "Windows 11 本机双账号登录及收发消息通过；截图与通话待验",
          canMultiLaunch: true,
        },
      }),
    );
    render(<App />);
    await ready();
    expect(screen.getByText("当前组合已通过双账号验收")).toBeInTheDocument();
    expect(
      screen.queryByText("此版本的多开能力尚未验证"),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/截图与通话待验/)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "启动至 2 个实例" }),
    ).toBeEnabled();
  });

  it("进度订阅失败会清除进度并显示错误", async () => {
    api.onProgress.mockRejectedValue({
      code: "event_error",
      message: "无法监听启动进度",
    });
    render(<App />);
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "启动至 2 个实例" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "无法监听启动进度",
    );
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(api.launch).not.toHaveBeenCalled();
  });

  it("浏览器演示有清楚的预览标识", async () => {
    api.preview = true;
    render(<App />);
    await ready();
    expect(screen.getByText("界面预览 · 不会启动微信")).toBeInTheDocument();
  });
});
