---
version: alpha
name: more-WeChat
description: A calm Chinese desktop utility for finding, launching, and diagnosing WeChat.
colors:
  primary: '#20483d'
  background: '#f6f7f2'
  surface: '#fffefa'
  text: '#263d36'
  muted: '#7b8880'
  border: '#e5e9df'
  accent: '#b9df9b'
typography:
  sans:
    fontFamily: "'Segoe UI', 'Microsoft YaHei', -apple-system, BlinkMacSystemFont, sans-serif"
  heading:
    fontFamily: "'Segoe UI', 'Microsoft YaHei', -apple-system, BlinkMacSystemFont, sans-serif"
    fontSize: '32px'
    lineHeight: '1.35'
rounded:
  DEFAULT: '9px'
  card: '16px'
spacing:
  panel-gap: '21px'
  page-max: '1080px'
components:
  launch-button: {}
  notice: {}
  stepper: {}
  scrollbar: {}
---

# more-WeChat Design System

## Overview

The north star is a quiet desktop tray companion: a pale workspace, pine controls, and overlapping conversation tiles. Chinese-speaking Windows and macOS users need one clear path from locating WeChat to understanding the launch result. The current user-approved brief establishes a single offline Chinese preview window; no Japan-market or Japanese locale scope is implied.

The signature is the small pair of offset conversation tiles, echoed by the five instance slots. Keep expression there; controls and errors use familiar native semantics. Avoid dashboard density, marketing statistics, neon effects, and unearned compatibility claims.

Runtime CSS in `src/styles.css` owns token values (model B). This document mirrors the accepted palette and explains its use. Existing implementation has one screen; no multi-screen UX contract is needed. Business behavior comes from the user-approved implementation plan and `src/types.ts`, with system capability decisions supplied by the Rust adapters.

## Colors

Pine is the primary launch action. Cream and pale mint distinguish configuration and detected status; amber identifies unverified compatibility, and warm red identifies errors. Always accompany semantic colors with icons and text. Use a visible green keyboard focus ring. The preview has one light theme; forced colors preserves system controls and operable scrollbars.

## Typography

Use installed system sans fonts with Microsoft YaHei and Apple fallbacks for Chinese. No remote font requests. Headings are 32px/1.35, controls 11–14px, compact supporting data 9–12px. Counts use tabular numerals. Technical paths truncate in the picker button and are fully present in accessible title and exported diagnostics. Body copy stays concise and action-oriented.

## Layout

The main window is 1040×780, minimum 760×640. The 1080px maximum shell contains a configuration card and a 270px status column, separated by 21px. At 800px, tighten padding and shrink the side column; at 640px, stack the workspace. The document owns vertical scrolling so results and long diagnostics remain reachable. No horizontal overflow is allowed at supported minimum width. Reserve scrollbar geometry with a stable gutter; loading retains control dimensions.

## Elevation & Depth

Use tonal surfaces, thin borders, and only restrained shadows on the configuration card and launch action. The conversation tile is the only decorative lifted object. Do not turn every information row into a floating card.

## Shapes

Controls use 9px corners, primary card 16px, and small status marks are circular. Lucide line icons use the existing 13–22px sizes; the WeChat-like conversation symbol uses filled shapes. Icons complement visible labels, and icon-only dismiss controls require Chinese accessible names.

## Components

| Capability | Canonical owner | Source of truth | Allowed variants | Verification |
|---|---|---|---|---|
| Scrollbar | Global `src/styles.css` | Runtime CSS tokens | document and nested overflow | browser smoke computed style |
| Toast | App notice region in `src/App.tsx` | native `AppError` and launch result | error, success, info | component tests and browser smoke |
| Form | App target stepper | target 1–5 in approved plan | decrease, increase, target-one shortcut | component tests |

`--pine`, `--muted`, `--line`, and `--green` map palette values into launch, metadata, divider, and accent roles. New common colors belong in the root token set. Scrollbar thumb, track, hover, and active tokens apply globally, with standards properties and WebKit fallbacks. Root font stack is the only typography owner.

Launch and refresh use native buttons with hover, visible focus, pressed, disabled, and busy states. A synchronous busy ref prevents duplicate dispatch before React renders. Register launch progress before invoking launch; unsubscribe even on failure. Errors remain native errors, never silently switching to the browser demo. Snapshot refresh occurs on initial load, explicit refresh, installation change, and launch completion; displayed counts say “上次检测”.

Settings save before updating the selected target. When `canMultiLaunch` is false, explain why multi-launch is unavailable and offer the single-instance action. Unverified status remains visible even when an adapter allows an attempted multi-launch. Progress and results use live regions; actual login is owned by WeChat. Native system file dialogs own file selection and save behavior. No destructive actions or permission prompts are added to this screen.

Motion is limited to a short button hover and a busy spinner. All motion stops under reduced-motion preference. Error and activity text wraps. Keep activity bounded to the most recent eight entries, showing at most four in the side column.

## Do's and Don'ts

- Keep install selection, target count, launch, and result in one window.
- Distinguish detected process success from account login and verified compatibility.
- Keep the browser preview label visible and its fake operations confined to browser preview.
- Do not poll expensive signature checks, terminate existing WeChat, or automatically request elevation.
- Do not fetch remote assets or use color alone to communicate state.
