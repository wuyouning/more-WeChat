import { useCallback, useEffect, useRef, useState } from "react";
import {
  ArrowDownToLine,
  ArrowRight,
  Check,
  CheckCheck,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  CircleHelp,
  FolderOpen,
  Info,
  LoaderCircle,
  MessageCircle,
  Minus,
  Monitor,
  Plus,
  RefreshCw,
  ShieldCheck,
  Terminal,
  X,
} from "lucide-react";
import { appApi, getAppError } from "./api";
import type { LaunchProgress, LaunchResult, Snapshot } from "./types";

type Busy =
  | "detecting"
  | "choosing"
  | "saving"
  | "launching"
  | "exporting"
  | null;
type Notice = {
  kind: "error" | "success" | "info";
  title: string;
  message: string;
};
type Activity = {
  id: number;
  time: string;
  message: string;
  kind: "success" | "error" | "info";
};

const busyLabels: Record<Exclude<Busy, null>, string> = {
  detecting: "正在检测微信",
  choosing: "正在选择安装位置",
  saving: "正在保存数量",
  launching: "正在启动微信",
  exporting: "正在导出诊断",
};

export default function App() {
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [target, setTarget] = useState(2);
  const [busy, setBusy] = useState<Busy>("detecting");
  const [notice, setNotice] = useState<Notice | null>(null);
  const [progress, setProgress] = useState<LaunchProgress | null>(null);
  const [result, setResult] = useState<LaunchResult | null>(null);
  const [activity, setActivity] = useState<Activity[]>([]);
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const busyRef = useRef<Busy>("detecting");
  const activityId = useRef(0);
  const mounted = useRef(true);

  const addActivity = useCallback(
    (message: string, kind: Activity["kind"] = "info") => {
      setActivity((items) =>
        [
          {
            id: ++activityId.current,
            time: new Date().toLocaleTimeString("zh-CN", {
              hour: "2-digit",
              minute: "2-digit",
            }),
            message,
            kind,
          },
          ...items,
        ].slice(0, 8),
      );
    },
    [],
  );

  const reportError = useCallback(
    (error: unknown) => {
      const detail = getAppError(error);
      const permission = /permission|access_denied/i.test(detail.code);
      setNotice({
        kind: "error",
        title: permission ? "权限不足" : "操作未完成",
        message: detail.message,
      });
      addActivity(detail.message, "error");
    },
    [addActivity],
  );

  useEffect(() => {
    mounted.current = true;
    let cancelled = false;
    appApi
      .snapshot()
      .then((state) => {
        if (cancelled) return;
        setSnapshot(state);
        setTarget(state.settings.targetCount);
        addActivity(
          state.installation
            ? "已找到微信，运行状态已更新"
            : "尚未找到微信，请选择安装位置",
        );
      })
      .catch((error: unknown) => {
        if (!cancelled) reportError(error);
      })
      .finally(() => {
        if (!cancelled) {
          busyRef.current = null;
          setBusy(null);
        }
      });
    return () => {
      cancelled = true;
      mounted.current = false;
    };
  }, [addActivity, reportError]);

  async function run(kind: Exclude<Busy, null>, action: () => Promise<void>) {
    if (busyRef.current) return;
    busyRef.current = kind;
    setBusy(kind);
    try {
      await action();
    } catch (error) {
      reportError(error);
    } finally {
      busyRef.current = null;
      if (mounted.current) setBusy(null);
    }
  }

  async function refresh() {
    await run("detecting", async () => {
      const state = await appApi.snapshot();
      setSnapshot(state);
      setNotice(null);
      addActivity(
        state.installation
          ? "检测完成，运行状态已更新"
          : "未找到微信，请手动选择安装位置",
      );
    });
  }

  async function chooseInstallation() {
    await run("choosing", async () => {
      const state = await appApi.selectInstallation(
        snapshot?.platform ?? "windows",
      );
      if (state) {
        setSnapshot(state);
        setNotice(null);
        setResult(null);
        addActivity("已更新微信安装位置");
      }
    });
  }

  async function changeTarget(value: number) {
    if (value < 1 || value > 5) return;
    await run("saving", async () => {
      await appApi.saveTarget(value);
      setTarget(value);
      setResult(null);
    });
  }

  async function launch() {
    await run("launching", async () => {
      setNotice(null);
      setResult(null);
      setProgress({
        stage: "preparing",
        message: "正在检查微信运行状态…",
        current: snapshot?.instances.length ?? 0,
        target,
      });
      let unlisten = () => {};
      try {
        unlisten = await appApi.onProgress((update) => {
          if (mounted.current) setProgress(update);
        });
        const outcome = await appApi.launch(target);
        setResult(outcome);
        addActivity(
          outcome.message,
          outcome.status === "failed"
            ? "error"
            : outcome.status === "partial"
              ? "info"
              : "success",
        );
        setSnapshot(await appApi.snapshot());
      } finally {
        unlisten();
        setProgress(null);
      }
    });
  }

  async function exportDiagnostics() {
    await run("exporting", async () => {
      const path = await appApi.exportDiagnostics();
      if (path) {
        setNotice({ kind: "success", title: "诊断信息已导出", message: path });
        addActivity("诊断信息已导出");
      }
    });
  }

  const count = snapshot?.instances.length ?? 0;
  const installed = Boolean(snapshot?.installation);
  const multiBlocked = target > 1 && !snapshot?.compatibility.canMultiLaunch;
  const outdated =
    snapshot?.clones.filter((clone) => clone.outdated).length ?? 0;
  const launchDisabled = Boolean(busy) || !installed || multiBlocked;
  const progressPercent = progress
    ? Math.min(
        100,
        Math.round((progress.current / Math.max(1, progress.target)) * 100),
      )
    : 0;

  return (
    <main className="app-shell">
      {appApi.preview && (
        <div className="preview-banner">
          <Monitor size={13} /> 界面预览 · 不会启动微信
        </div>
      )}
      <header className="app-header">
        <a className="brand" href="#main" aria-label="more-WeChat 主页">
          <span className="brand-symbol">
            <MessageCircle size={22} strokeWidth={2.2} />
            <Plus className="brand-plus" size={12} strokeWidth={3} />
          </span>
          <span>
            more<span className="brand-dash">—</span>WeChat
          </span>
          <span className="version-label">PREVIEW</span>
        </a>
        <span className="local-label">
          <span className="status-dot" /> 本地运行 · 离线可用
        </span>
      </header>

      <section className="intro" id="main">
        <div>
          <p className="eyebrow">YOUR WECHAT, WITH MORE ROOM</p>
          <h1>
            多一点空间，
            <br className="mobile-break" />
            从容切换。
          </h1>
          <p className="intro-copy">找到微信，选择数量，剩下的交给我们。</p>
        </div>
        <div className="intro-illustration" aria-hidden="true">
          <span className="orbit-dot" />
          <div className="chat-back">
            <MessageCircle size={34} />
          </div>
          <div className="chat-front">
            <MessageCircle size={38} />
            <span>+</span>
          </div>
        </div>
      </section>

      {notice && (
        <div
          className={`notice notice-${notice.kind}`}
          role={notice.kind === "error" ? "alert" : "status"}
        >
          <CircleAlert size={18} />
          <div>
            <strong>{notice.title}</strong>
            <p>{notice.message}</p>
          </div>
          <button
            className="icon-button notice-close"
            aria-label="关闭提示"
            onClick={() => setNotice(null)}
          >
            <X size={16} />
          </button>
        </div>
      )}

      <div className="workspace-grid">
        <section className="launcher-card" aria-label="微信启动设置">
          <div className="card-heading">
            <span className="section-index">01</span>
            <h2>
              {installed ? "微信已就位" : "找到你的微信"}
              {!installed && (
                <span className="heading-status">
                  {busy === "detecting" ? "检测中" : "待选择"}
                </span>
              )}
            </h2>
            <button
              className="text-button refresh-button"
              disabled={Boolean(busy)}
              onClick={() => void refresh()}
            >
              <RefreshCw
                size={14}
                className={busy === "detecting" ? "spin" : ""}
              />
              重新检测
            </button>
          </div>
          <div className="installation-row">
            <div
              className={`wechat-icon ${!installed ? "wechat-icon-muted" : ""}`}
            >
              <MessageCircle size={28} fill="currentColor" strokeWidth={0} />
              <MessageCircle
                size={22}
                className="second-chat"
                fill="currentColor"
                strokeWidth={0}
              />
            </div>
            <div className="installation-name">
              <strong>{installed ? "微信 WeChat" : "选择你的微信"}</strong>
              <span>
                {installed
                  ? `版本 ${snapshot?.installation?.version || "未知"}`
                  : "自动检测常用安装位置，也可手动选择"}
              </span>
            </div>
            <span
              className={`pill ${installed ? "pill-green" : "pill-neutral"}`}
            >
              <span className="status-dot" />
              {installed ? "已找到" : "未找到"}
            </span>
          </div>
          <button
            className="path-button"
            disabled={Boolean(busy)}
            onClick={() => void chooseInstallation()}
            title={snapshot?.installation?.path ?? "手动选择微信安装位置"}
          >
            <FolderOpen size={15} />
            <span>
              {snapshot?.installation?.path ?? "手动选择微信安装位置"}
            </span>
            <ChevronRight size={15} />
          </button>
          <div className="divider" />
          <div className="card-heading target-heading">
            <span className="section-index">02</span>
            <h2 id="target-label">想同时打开几个？</h2>
          </div>
          <div className="target-row">
            <div className="target-description">
              <strong>目标总数量</strong>
              <p>已有实例会保留，只补齐不足的数量。</p>
            </div>
            <div
              className="stepper"
              role="group"
              aria-labelledby="target-label"
            >
              <button
                aria-label="减少目标数量"
                disabled={Boolean(busy) || target <= 1}
                onClick={() => void changeTarget(target - 1)}
              >
                <Minus size={18} />
              </button>
              <output aria-label="目标总数量" aria-live="polite">
                {target}
              </output>
              <button
                aria-label="增加目标数量"
                disabled={Boolean(busy) || target >= 5}
                onClick={() => void changeTarget(target + 1)}
              >
                <Plus size={18} />
              </button>
            </div>
          </div>
          <div className="capacity-hint">
            <span>1–5 个实例</span>
            <span>
              <span className="small-running-dot" /> 当前运行 {count} 个
            </span>
          </div>

          {snapshot?.compatibility && installed && (
            <div
              className={`compatibility-note ${snapshot.compatibility.status === "verified" ? "compatibility-verified" : ""}`}
            >
              <ShieldCheck size={16} />
              <div>
                <strong>
                  {snapshot.compatibility.status === "verified"
                    ? "此版本已完成实机验证"
                    : "此版本的多开能力尚未验证"}
                </strong>
                <p>{snapshot.compatibility.message}</p>
                {multiBlocked && (
                  <button
                    className="inline-button"
                    disabled={Boolean(busy)}
                    onClick={() => void changeTarget(1)}
                  >
                    先启动一个微信 <ArrowRight size={12} />
                  </button>
                )}
              </div>
            </div>
          )}
          {outdated > 0 && (
            <div className="outdated-note">
              <Info size={15} />
              <span>
                {outdated}{" "}
                个受管理副本需要更新。启动时仅刷新已停止的副本，保留数据。
              </span>
            </div>
          )}

          <button
            className="launch-button"
            disabled={launchDisabled}
            onClick={() => void launch()}
          >
            {busy === "launching" ? (
              <LoaderCircle size={20} className="spin" />
            ) : (
              <span className="launch-symbol">
                <Plus size={18} />
              </span>
            )}
            <span>
              {busy === "launching"
                ? "正在启动，请稍候…"
                : target === 1
                  ? "启动微信"
                  : `启动至 ${target} 个实例`}
            </span>
            {busy !== "launching" && <ArrowRight size={20} />}
          </button>
          <p className="launch-footnote">
            <ShieldCheck size={13} /> 保留当前微信 · 不自动提权 · 登录由微信完成
          </p>
          {busy && (
            <span className="sr-only" role="status">
              {busyLabels[busy]}
            </span>
          )}
          {progress && (
            <div className="progress-section" aria-live="polite">
              <div>
                <span>{progress.message}</span>
                <span>
                  {progress.current}/{progress.target}
                </span>
              </div>
              <div
                className="progress-track"
                role="progressbar"
                aria-label="微信启动进度"
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={progressPercent}
              >
                <span style={{ width: `${progressPercent}%` }} />
              </div>
            </div>
          )}
          {result && (
            <div
              className={`launch-result result-${result.status}`}
              role={result.status === "failed" ? "alert" : "status"}
            >
              {result.status === "success" || result.status === "unchanged" ? (
                <CheckCheck size={18} />
              ) : (
                <CircleAlert size={18} />
              )}
              <div>
                <strong>
                  {result.status === "success"
                    ? "微信已启动"
                    : result.status === "unchanged"
                      ? "已达到目标数量"
                      : result.status === "partial"
                        ? "部分启动成功"
                        : "微信未能启动"}
                </strong>
                <p>{result.message}</p>
                <p>
                  当前 {result.after} 个 / 目标 {result.target} 个
                </p>
                {result.errors.length > 0 && (
                  <ul>
                    {result.errors.map((error, index) => (
                      <li key={index}>{error}</li>
                    ))}
                  </ul>
                )}
              </div>
            </div>
          )}
        </section>

        <aside className="side-column">
          <section className="status-card" aria-label="微信运行状态">
            <div className="side-heading">
              <span className="section-kicker">LAST CHECK · 上次检测</span>
              <span className={`live-dot ${busy ? "live-dot-busy" : ""}`} />
            </div>
            <div className="running-count">
              <span>{count}</span>
              <span>个运行中的实例</span>
            </div>
            <div className="instance-tiles" aria-hidden="true">
              {Array.from({ length: 5 }, (_, index) => (
                <span
                  className={
                    index < count ? "instance-tile active" : "instance-tile"
                  }
                  key={index}
                >
                  {index < count ? (
                    <MessageCircle size={17} />
                  ) : (
                    <Plus size={15} />
                  )}
                </span>
              ))}
            </div>
            <p className="status-caption">
              {count >= target
                ? "已达到你设置的目标数量"
                : count === 0
                  ? "准备好后，开启你的第一个实例"
                  : `再启动 ${Math.max(0, target - count)} 个，即可达到目标`}
            </p>
            <div className="status-divider" />
            <p className="status-assurance">
              <Check size={14} /> 只统计微信主进程
            </p>
            <p className="status-assurance">
              <Check size={14} /> 启动后确认进程稳定运行
            </p>
          </section>

          <section className="activity-card" aria-label="操作记录">
            <div className="side-heading">
              <h2>最近动态</h2>
              <span className="tiny-label">本次打开</span>
            </div>
            {activity.length === 0 ? (
              <p className="empty-activity">正在读取运行状态…</p>
            ) : (
              <ol className="activity-list">
                {activity.slice(0, 4).map((item) => (
                  <li key={item.id}>
                    <span className={`activity-dot activity-${item.kind}`} />
                    <div>
                      <p>{item.message}</p>
                      <time>{item.time}</time>
                    </div>
                  </li>
                ))}
              </ol>
            )}
          </section>
          <button
            className="help-card"
            disabled={Boolean(busy)}
            onClick={() => void exportDiagnostics()}
          >
            <span className="help-icon">
              <CircleHelp size={19} />
            </span>
            <span>
              <strong>遇到了问题？</strong>
              <small>导出诊断信息，方便排查</small>
            </span>
            <ArrowDownToLine size={17} />
          </button>
        </aside>
      </div>

      <section className="diagnostics-section">
        <button
          className="diagnostics-toggle"
          aria-expanded={diagnosticsOpen}
          onClick={() => setDiagnosticsOpen(!diagnosticsOpen)}
        >
          <Terminal size={14} /> 诊断详情
          <ChevronDown size={14} className={diagnosticsOpen ? "rotate" : ""} />
        </button>
        {diagnosticsOpen && (
          <div className="diagnostics-panel">
            <dl>
              <div>
                <dt>操作系统</dt>
                <dd>{snapshot?.osVersion ?? "等待检测"}</dd>
              </div>
              <div>
                <dt>兼容状态</dt>
                <dd>
                  {snapshot?.compatibility.status === "verified"
                    ? "已验证"
                    : "未验证"}
                </dd>
              </div>
              <div>
                <dt>主进程 PID</dt>
                <dd>
                  {snapshot?.instances
                    .map((instance) => instance.pid)
                    .join("、") || "无"}
                </dd>
              </div>
            </dl>
            {snapshot?.notes.map((note, index) => (
              <p key={index}>{note}</p>
            ))}
            <p>诊断导出会隐藏用户目录中的用户名，不包含微信聊天内容。</p>
            <button
              className="text-button"
              disabled={Boolean(busy)}
              onClick={() => void exportDiagnostics()}
            >
              <ArrowDownToLine size={14} />
              导出诊断信息
            </button>
          </div>
        )}
      </section>
      <footer className="app-footer">
        <span>少一点繁琐，多一点从容。</span>
        <span>
          more-WeChat <span className="footer-separator">/</span> v0.2 预览版
        </span>
      </footer>
    </main>
  );
}
