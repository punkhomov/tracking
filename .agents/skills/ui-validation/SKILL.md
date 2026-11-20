---
name: ui-validation
description: Verify Tracking UI behavior and screenshots using core command scenarios, egui tests, and native or browser smoke tests.
---

# Tracking UI validation

Read [architecture](../../../docs/architecture.md) for the command and control boundaries and [UI guidelines](../../../docs/ui-guidelines.md) for layout and accessibility expectations. Use the commands and harnesses available in the current checkout.

- Run state transitions against the core with fixed times and test records. Assert the observable result and any state that must survive a restart.
- Use egui's test harness or the available UI runner for keyboard navigation, focus, clicks and feedback. Exercise widget input when the behavior depends on routing. Query by accessible name where the harness supports it.
- Use an isolated profile for screenshots and pin the viewport, theme, scaling and clock. Wait for the requested state and its rendered frame before capturing. Inspect the image and, for interaction changes, the accessibility tree where available.
- For browser-specific capabilities, run the WASM build in a browser. Verify the affected scenario, including reload for persistence and denied permissions when relevant. A build checks compatibility, and a smoke test checks runtime behavior.
- Use synthetic data in captures and logs. Report which targets, interactions and accessibility checks ran.
