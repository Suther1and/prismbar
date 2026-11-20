# PRD — Addendum (Технический стек и архитектурные соображения)

*Вынесено из основного PRD согласно дисциплине BMAD (техническая реализация фиксируется детально на шаге `bmad-architecture`, здесь — предварительная проверка осуществимости на текущей машине).*

## Проверенная среда на машине разработчика
- OS: Windows 11 Home Build 26200 (24H2/25H2).
- Rust `1.98.1` + Cargo + Visual Studio 2022 Build Tools (MSVC).
- Node.js `v24.16.0`, `pnpm`, `bun`.
- WebView2 Runtime `157.0.4341.60` (встроен в Windows 11).
- `.NET SDK` не установлен (только runtime 8.0.19).

## Рекомендуемый стек (для утверждения в `bmad-architecture`)
- **Ядро и системная интеграция:** **Tauri v2 + Rust (`windows` crate)** — нативный, сверхбыстрый и безопасный бэкенд без сборщика мусора, минимальный размер `.exe` (~6–12 МБ вместо ~150 МБ у Electron), прямой доступ к Win32/COM/WinRT API (`SHAppBarMessage`, `EnumWindows`, `RegisterShellHookWindow`, `WH_KEYBOARD_LL`, `DwmSetWindowAttribute` Acrylic/Mica, `IAudioEndpointVolume` WASAPI, `GlobalSystemMediaTransportControlsSessionManager` WinRT, `GetKeyboardLayout`, `ShellExecuteW`, извлечение иконок `SHGetFileInfoW` / `ExtractIconExW`).
- **UI и анимации:** **React 19 + TypeScript + Tailwind CSS v4 + Motion (Framer Motion / CSS Spring Physics)** в системном WebView2 с прозрачным окном (`transparent: true`) и нативным DWM-размытием (`window-vibrancy` / `DwmSetWindowAttribute`).
- **Почему именно Tauri v2 + Rust + React/Motion:**
  1. Визуальный потолок мирового уровня (градиенты, микро-анимации, пружинная физика, тончайшие стеклянные бордеры, 120–240 FPS на GPU).
  2. Нативная скорость системного слоя на Rust (мгновенный перехват `Win`, мгновенное перечисление окон, нулевые фризы, нативный watchdog-процесс).
  3. Сборка в готовый `.exe` прямо на этой машине без установки гигабайтных SDK.
  4. **100% соответствие HTML UX-превью финальному приложению:** движок рендеринга и CSS/JS анимации в превью и в собранном `.exe` (WebView2 Chromium) — **один и тот же**, поэтому визуал и анимации из превью перенесутся в `.exe` один-в-один!
