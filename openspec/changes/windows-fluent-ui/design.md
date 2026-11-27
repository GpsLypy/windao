## Context

The workbench and capsule already render with egui and the capsule supports multiple independent skins.

## Goals / Non-Goals

- Use consistent control states in light and dark themes.
- Keep settings usable in narrow windows and through the keyboard.
- Keep the existing storage and native integration paths.

## Decisions

- Apply neutral secondary button styles through egui widget visuals and use blue for primary actions.
- Reuse one navigation painter for the workbench, settings, and capsule launcher. Derive its colors from the current surface.
- Pair each visual switch with its current state and accessible name. Give settings with exclusive choices a labeled dropdown.
- Use Segoe UI when available and the existing CJK fallback.

## Risks / Trade-offs

- Offscreen interaction verifies application behavior. Native display scaling and accessibility integration require a Windows session.
