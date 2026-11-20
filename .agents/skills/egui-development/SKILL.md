---
name: egui-development
description: Implement or review Tracking screens, widgets, and egui input handling shared by native eframe and WASM builds.
---

# Tracking UI development

Read [architecture](../../../docs/architecture.md) and [UI guidelines](../../../docs/ui-guidelines.md). Check [the stack](../../../docs/tech-stack.md) when changing dependencies, platform features or the rendering path.

- Start with the screen's existing components and theme tokens. Keep egui IDs stable across sorting and filtering by deriving them from record identity. Keep drafts and focus in UI state and send committed edits as commands to the core.
- Route file selection, clipboard capabilities beyond egui's built-ins, notification permissions and other platform effects through the existing adapters. Surface the result when the operation completes.
- Plan a repaint for the next visible timer change. Request one immediately after interaction or completion of a background effect. Cache summaries by data revision and filter when recomputing on every frame would be expensive.
- Check both entry points for changed shared code. For screenshots, input tests and browser-specific behavior, use [ui-validation](../ui-validation/SKILL.md).
