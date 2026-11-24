# OpenBilling desktop

Slint front-end for Windows, macOS and Linux. See the [project README](../README.md).

- `ui/` – Slint markup: `theme.slint` (design tokens and icons), `globals.slint`
  (data structs, state and the `Api` callbacks), `components/`, `screens/`, and
  `windows/MainWindow.slint` (shell, navigation and dialogs).
- `src/` – one controller per area (`auth_ui`, `orders_ui`, `templates_ui`, …).
  Each registers its `Api` callbacks and calls into `app-core`.
