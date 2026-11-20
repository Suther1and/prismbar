---
type: initiative
title: "TestBar — Windows 11 Taskbar & Start Menu Replacement"
parent: none
status: planning
sources:
  - brief-testbar/brief-testbar.md
  - prd-testbar/prd-testbar.md
companions:
  - ux-testbar/ux-testbar.md
  - ux-testbar/DESIGN.md
  - ux-testbar/EXPERIENCE.md
  - architecture-testbar/architecture-testbar.md
---

# Initiative: TestBar — Замена панели задач и «Пуск» для Windows 11

## Goal
Создать с нуля готовое настольное приложение (`testbar.exe`) для Windows 11, полностью заменяющее стандартную нижнюю панель задач и меню «Пуск», в точном соответствии с утверждённым UX (`ux-testbar/mockups/complete-ux.html`).

## Requirements
- **R1** (AppBar & Safe Native Hide): Резервировать рабочую область экрана (`SHAppBarMessage`), безопасно скрывать `Shell_TrayWnd` / `Shell_SecondaryTrayWnd` без DLL-инъекций и гарантированно восстанавливать штатную панель при выходе, падении (внешний watchdog) или по хоткею `Ctrl+Alt+Shift+Esc`.
- **R2** (Taskbar Windows & Context Menu): Отображать закреплённые и активные окна с нативными иконками, индикацией состояний, строгим удержанием ховера внутри капсулы, живыми превью окон при наведении (выбор / закрытие) и полным правым кликом (Jump List, новое окно, от админа, закрепить/открепить, закрыть, **Завершить задачу**).
- **R3** (Fullscreen Auto-Hide): Автоматически скрывать панель при полноэкранных играх/видео (`SHQueryUserNotificationState`) и возвращать при выходе.
- **R4** (Compact 40-Icon Tray, Sound/Mixer, Layout, Calendar): Компактная 6-колоночная сетка фоновых иконок (`^`) под 30–40 приложений с тултипом и ПКМ; переключатель раскладки (`RU`/`EN`); звук/сеть с регулировкой колёсиком, OSD, переключением устройств вывода (WASAPI/IPolicyConfig) и микшером приложений; часы и календарь.
- **R5** (Start Menu — 4 Adaptive Spaces & Universal Pinning): Запуск по кнопке и одиночному `Win` (`WH_KEYBOARD_LL`, не ломая `Win+<клавиша>`); пространства **Личное · Работа · Студия · Игры** + пользовательские; режим редактирования (перетаскивание + открепление); универсальное закрепление программ, папок, файлов и URL; сохранение конфигурации в `%APPDATA%\TestBar\config.json`.
- **R6** (Instant Type-Anywhere Search): Мгновенный локальный поиск по программам, играм, папкам и файлам при вводе с клавиатуры без потери первых символов и с поддержкой ошибочной раскладки RU↔EN.
- **R7** (Real Hardware, Media, Clipboard & Games): Живые данные SMTC (медиаплеер), живой буфер обмена (`AddClipboardFormatListener`), понятная телеметрия ПК (CPU, RAM/64, RTX 4090 NVML temp/load/VRAM/24, топ потребителей памяти с завершением процесса), батарея Bluetooth-периферии, автоскан библиотеки Steam с реальными обложками и игровые связки.
- **R8** (Standalone `.exe` Delivery): Локальная сборка Tauri v2 + Rust + React 19 в готовый рабочий `.exe` с высокой плавностью (60–240 FPS) и режимами панели «Док» / «Полоса».

## Constraints
- Никаких DLL-инъекций и модификации `explorer.exe`.
- Строго тёмная тема, без отклонённых виджетов (VPN, калькулятор, заметки, мониторные кнопки, «Режимы работы», эмодзи, текстовые подсказки в UI).
- Полностью локальная сборка на Windows 11 (Rust + MSVC + Node + WebView2).
