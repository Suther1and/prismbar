use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PinItem {
    pub id: String,
    /// "app" | "folder" | "file" | "url"
    pub kind: String,
    pub name: String,
    pub subtitle: String,
    pub target: String,
    #[serde(default)]
    pub alt_action: String,
    #[serde(default)]
    pub accent_rgb: String,
    #[serde(default)]
    pub icon_key: String,
    #[serde(default)]
    pub icon_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GameCombo {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub targets: Vec<String>,
    pub accent_rgb: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CustomSpace {
    pub id: String,
    pub name: String,
    pub accent_hex: String,
    pub accent_rgb: String,
    pub items: Vec<PinItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestBarConfig {
    pub bar_mode: String, // "dock" | "edge"
    pub autostart: bool,
    pub taskbar_pins: Vec<PinItem>,
    pub personal_pins: Vec<PinItem>,
    pub personal_links: Vec<PinItem>,
    pub work_folders: Vec<PinItem>,
    pub work_apps: Vec<PinItem>,
    pub studio_pins: Vec<PinItem>,
    pub studio_tools: Vec<PinItem>,
    pub game_launchers: Vec<PinItem>,
    pub game_combos: Vec<GameCombo>,
    pub custom_spaces: Vec<CustomSpace>,
}

fn find_first_existing(candidates: &[PathBuf]) -> Option<String> {
    for p in candidates {
        if p.exists() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

fn user_home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(r"C:\Users\Public"))
}

fn local_appdata() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| user_home().join(r"AppData\Local"))
}

fn roaming_appdata() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| user_home().join(r"AppData\Roaming"))
}

fn detect_app_target(name_key: &str) -> String {
    let local = local_appdata();
    let roaming = roaming_appdata();
    let pf = PathBuf::from(r"C:\Program Files");
    let pf86 = PathBuf::from(r"C:\Program Files (x86)");

    let candidates: Vec<PathBuf> = match name_key {
        "explorer" => vec![PathBuf::from(r"C:\Windows\explorer.exe")],
        "zen" => vec![
            pf.join(r"Zen Browser\zen.exe"),
            local.join(r"Zen Browser\zen.exe"),
            pf.join(r"Mozilla Firefox\firefox.exe"),
        ],
        "telegram" => vec![
            roaming.join(r"Telegram Desktop\Telegram.exe"),
            local.join(r"Programs\Telegram Desktop\Telegram.exe"),
        ],
        "spark" => vec![
            local.join(r"SparkDesktop\SparkDesktop.exe"),
            local.join(r"Programs\SparkDesktop\SparkDesktop.exe"),
        ],
        "obsidian" => vec![
            local.join(r"Programs\obsidian\Obsidian.exe"),
            local.join(r"Obsidian\Obsidian.exe"),
        ],
        "claude" => vec![local.join(r"AnthropicClaude\claude.exe")],
        "yandex" => vec![
            local.join(r"Yandex\YandexBrowser\Application\browser.exe"),
        ],
        "ymusic" => vec![
            local.join(r"Programs\YandexMusic\Яндекс Музыка.exe"),
            local.join(r"Programs\YandexMusic\YandexMusic.exe"),
        ],
        "discord" => vec![
            local.join(r"Discord\Update.exe"),
        ],
        "obs" => vec![
            pf.join(r"obs-studio\bin\64bit\obs64.exe"),
        ],
        "antigravity" => vec![
            local.join(r"Programs\Antigravity\Antigravity.exe"),
        ],
        "cursor" => vec![
            local.join(r"Programs\cursor\Cursor.exe"),
        ],
        "vscode" => vec![
            local.join(r"Programs\Microsoft VS Code\Code.exe"),
            pf.join(r"Microsoft VS Code\Code.exe"),
        ],
        "photoshop" => vec![
            pf.join(r"Adobe\Adobe Photoshop 2025\Photoshop.exe"),
            pf.join(r"Adobe\Adobe Photoshop 2024\Photoshop.exe"),
        ],
        "lightroom" => vec![
            pf.join(r"Adobe\Adobe Lightroom Classic\Lightroom.exe"),
        ],
        "figma" => vec![
            local.join(r"Figma\Figma.exe"),
        ],
        "unity" => vec![
            pf.join(r"Unity Hub\Unity Hub.exe"),
        ],
        "steam" => vec![
            PathBuf::from(r"D:\steam\steam.exe"),
            pf86.join(r"Steam\steam.exe"),
        ],
        "battlenet" => vec![
            pf86.join(r"Battle.net\Battle.net Launcher.exe"),
        ],
        "warp" => vec![
            local.join(r"Programs\Warp\warp.exe"),
        ],
        "wt" => vec![
            local.join(r"Microsoft\WindowsApps\wt.exe"),
            PathBuf::from(r"C:\Windows\System32\cmd.exe"),
        ],
        "wiztree" => vec![
            pf.join(r"WizTree\WizTree64.exe"),
        ],
        _ => vec![],
    };

    find_first_existing(&candidates).unwrap_or_else(|| match name_key {
        "explorer" => r"C:\Windows\explorer.exe".to_string(),
        "steam" => "steam://open/main".to_string(),
        "ymusic" => "https://music.yandex.ru".to_string(),
        "claude" => "https://claude.ai".to_string(),
        "figma" => "https://figma.com".to_string(),
        _ => String::new(),
    })
}

fn pin(
    id: &str,
    kind: &str,
    name: &str,
    subtitle: &str,
    target: String,
    alt_action: &str,
    accent_rgb: &str,
    icon_key: &str,
) -> PinItem {
    PinItem {
        id: id.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        subtitle: subtitle.to_string(),
        target,
        alt_action: alt_action.to_string(),
        accent_rgb: accent_rgb.to_string(),
        icon_key: icon_key.to_string(),
        icon_data: None,
    }
}

impl Default for TestBarConfig {
    fn default() -> Self {
        let home = user_home();
        let downloads = dirs::download_dir()
            .unwrap_or_else(|| home.join("Downloads"))
            .to_string_lossy()
            .to_string();
        let documents = dirs::document_dir()
            .unwrap_or_else(|| home.join("Documents"))
            .to_string_lossy()
            .to_string();
        let desktop = dirs::desktop_dir()
            .unwrap_or_else(|| home.join("Desktop"))
            .to_string_lossy()
            .to_string();
        let pictures = dirs::picture_dir()
            .unwrap_or_else(|| home.join("Pictures"))
            .to_string_lossy()
            .to_string();
        let testbar_dir = PathBuf::from(&documents)
            .join("testbar")
            .to_string_lossy()
            .to_string();

        Self {
            bar_mode: "dock".to_string(),
            autostart: false,
            taskbar_pins: vec![
                pin("explorer", "app", "Проводник", "Файлы и диски", detect_app_target("explorer"), "Новое окно", "245,158,11", "explorer"),
                pin("zen", "app", "Zen Browser", "Браузер", detect_app_target("zen"), "Новое окно", "249,115,22", "zen"),
                pin("telegram", "app", "Telegram", "Мессенджер", detect_app_target("telegram"), "Избранное", "56,189,248", "telegram"),
                pin("spark", "app", "Spark Mail", "Почта", detect_app_target("spark"), "Новое письмо", "59,130,246", "spark"),
                pin("antigravity", "app", "Antigravity", "IDE", detect_app_target("antigravity"), "Новый проект", "56,189,248", "antigravity"),
                pin("vscode", "app", "VS Code", "Редактор", detect_app_target("vscode"), "Новое окно", "56,189,248", "vscode"),
                pin("ymusic", "app", "Яндекс Музыка", "Плеер", detect_app_target("ymusic"), "Моя волна", "250,204,21", "ymusic"),
            ],
            personal_pins: vec![
                pin("zen", "app", "Zen Browser", "Основной браузер", detect_app_target("zen"), "Приватное окно", "249,115,22", "zen"),
                pin("telegram", "app", "Telegram", "Мессенджер", detect_app_target("telegram"), "Избранное", "56,189,248", "telegram"),
                pin("spark", "app", "Spark Mail", "Почта", detect_app_target("spark"), "Новое письмо", "59,130,246", "spark"),
                pin("explorer", "app", "Проводник", "Файлы и диски", detect_app_target("explorer"), "Новое окно", "245,158,11", "explorer"),
                pin("obsidian", "app", "Второй мозг", "Obsidian", detect_app_target("obsidian"), "Новая заметка", "139,92,246", "obsidian"),
                pin("claude", "app", "Claude", "Ассистент", detect_app_target("claude"), "Новый чат", "217,119,87", "claude"),
                pin("yandex", "app", "Яндекс", "Браузер", detect_app_target("yandex"), "Инкогнито", "239,68,68", "yandex"),
                pin("discord", "app", "Discord", "Голос и чаты", detect_app_target("discord"), "Каналы", "99,102,241", "discord"),
                pin("ymusic", "app", "Я.Музыка", "Плеер", detect_app_target("ymusic"), "Моя волна", "250,204,21", "ymusic"),
                pin("obs", "app", "OBS Studio", "Запись экрана", detect_app_target("obs"), "От админа", "148,163,184", "obs"),
                pin("wt", "app", "Терминал", "Консоль", detect_app_target("wt"), "От админа", "52,211,153", "warp"),
            ],
            personal_links: vec![
                pin("lnk-downloads", "folder", "Загрузки", "Папка", downloads.clone(), "Скопировать путь", "56,189,248", "folder"),
                pin("lnk-docs", "folder", "Документы", "Папка", documents.clone(), "Скопировать путь", "245,158,11", "folder"),
                pin("lnk-pics", "folder", "Изображения", "Скриншоты", pictures.clone(), "Скопировать путь", "139,92,246", "image"),
                pin("lnk-desktop", "folder", "Рабочий стол", "Файлы", desktop.clone(), "Скопировать путь", "52,211,153", "folder"),
            ],
            work_folders: vec![
                pin("wf-dl", "folder", "Загрузки", "Свежие файлы", downloads, "Открыть в терминале", "245,158,11", "folder"),
                pin("wf-docs", "folder", "Документы", "Основная папка", documents.clone(), "Открыть в терминале", "245,158,11", "folder"),
                pin("wf-testbar", "folder", "testbar", "Проект · сейчас", testbar_dir, "Открыть в терминале", "56,189,248", "folder"),
                pin("wf-desktop", "folder", "Рабочий стол", "Файлы и ярлыки", desktop, "Открыть в терминале", "56,189,248", "folder"),
                pin("wf-pics", "folder", "Скриншоты", "Изображения", pictures, "Открыть в терминале", "139,92,246", "folder"),
                pin("wf-home", "folder", "Профиль", "Домашняя папка", home.to_string_lossy().to_string(), "Открыть в терминале", "52,211,153", "folder"),
            ],
            work_apps: vec![
                pin("wa-ag", "app", "Antigravity", "IDE", detect_app_target("antigravity"), "Новое окно", "56,189,248", "antigravity"),
                pin("wa-cur", "app", "Cursor", "Код", detect_app_target("cursor"), "Новое окно", "226,232,240", "cursor"),
                pin("wa-vsc", "app", "VS Code", "Редактор", detect_app_target("vscode"), "Новое окно", "56,189,248", "vscode"),
                pin("wa-wt", "app", "Терминал", "Консоль", detect_app_target("wt"), "От админа", "52,211,153", "warp"),
            ],
            studio_pins: vec![
                pin("photoshop", "app", "Photoshop 2025", "Графика и ретушь", detect_app_target("photoshop"), "От админа", "49,168,255", "photoshop"),
                pin("obsidian-s", "app", "Obsidian", "База знаний", detect_app_target("obsidian"), "Новая заметка", "139,92,246", "obsidian"),
                pin("figma", "app", "Figma", "Интерфейсы", detect_app_target("figma"), "Открыть", "168,85,247", "figma"),
                pin("lightroom", "app", "Lightroom", "Classic", detect_app_target("lightroom"), "Каталог", "56,189,248", "lightroom"),
                pin("unity", "app", "Unity Hub", "Проекты 3D", detect_app_target("unity"), "Открыть", "226,232,240", "unity"),
                pin("obs-s", "app", "OBS Studio", "Захват видео", detect_app_target("obs"), "От админа", "148,163,184", "obs"),
                pin("antigravity-s", "app", "Antigravity", "IDE", detect_app_target("antigravity"), "Новый проект", "56,189,248", "antigravity"),
                pin("cursor-s", "app", "Cursor", "Редактор кода", detect_app_target("cursor"), "Новое окно", "226,232,240", "cursor"),
                pin("vscode-s", "app", "VS Code", "Workspace", detect_app_target("vscode"), "Новое окно", "56,189,248", "vscode"),
                pin("claude-s", "app", "Claude", "AI-ассистент", detect_app_target("claude"), "Новый чат", "217,119,87", "claude"),
                pin("warp-s", "app", "Warp", "Терминал", detect_app_target("warp"), "От админа", "52,211,153", "warp"),
            ],
            studio_tools: vec![
                pin("st-snip", "app", "Ножницы", "Скриншот", "ms-screenclip:".to_string(), "Открыть", "52,211,153", "scissors"),
                pin("st-wt", "app", "Терминал", "Консоль", detect_app_target("wt"), "От админа", "52,211,153", "warp"),
                pin("st-wiz", "app", "WizTree", "Диски", detect_app_target("wiztree"), "От админа", "56,189,248", "disk"),
                pin("st-tm", "app", "Диспетчер", "Процессы", r"C:\Windows\System32\Taskmgr.exe".to_string(), "От админа", "129,140,248", "cpu"),
            ],
            game_launchers: vec![
                pin("gl-steam", "app", "Steam", "Библиотека", detect_app_target("steam"), "Открыть", "56,189,248", "steam"),
                pin("gl-bnet", "app", "Battle.net", "Лаунчер", detect_app_target("battlenet"), "Открыть", "14,165,233", "battlenet"),
                pin("gl-discord", "app", "Discord", "Голос", detect_app_target("discord"), "Открыть", "99,102,241", "discord"),
                pin("gl-obs", "app", "OBS Studio", "Запись", detect_app_target("obs"), "Открыть", "148,163,184", "obs"),
            ],
            game_combos: vec![
                GameCombo {
                    id: "combo-poe2".to_string(),
                    name: "Path of Exile 2 + Steam".to_string(),
                    subtitle: "Steam · запуск игры".to_string(),
                    targets: vec!["steam://rungameid/2694490".to_string()],
                    accent_rgb: "251,113,133".to_string(),
                },
                GameCombo {
                    id: "combo-dota2".to_string(),
                    name: "Dota 2 + Discord".to_string(),
                    subtitle: "Steam + голосовой чат".to_string(),
                    targets: vec![
                        "steam://rungameid/570".to_string(),
                        detect_app_target("discord"),
                    ],
                    accent_rgb: "56,189,248".to_string(),
                },
            ],
            custom_spaces: vec![],
        }
    }
}

pub fn config_path() -> PathBuf {
    roaming_appdata().join("TestBar").join("config.json")
}

pub fn load_config() -> TestBarConfig {
    let path = config_path();
    if path.exists() {
        if let Ok(raw) = fs::read_to_string(&path) {
            if let Ok(cfg) = serde_json::from_str::<TestBarConfig>(&raw) {
                return cfg;
            }
        }
    }
    let cfg = TestBarConfig::default();
    let _ = save_config(&cfg);
    cfg
}

pub fn save_config(cfg: &TestBarConfig) -> Result<(), String> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create_dir_all: {e}"))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| format!("serialize: {e}"))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json).map_err(|e| format!("write tmp: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("rename tmp: {e}"))?;
    Ok(())
}

pub fn add_pin_to_space(cfg: &mut TestBarConfig, space: &str, item: PinItem) {
    match space {
        "personal" => cfg.personal_pins.push(item),
        "personal_links" => cfg.personal_links.push(item),
        "work" => {
            if item.kind == "app" {
                cfg.work_apps.push(item);
            } else {
                cfg.work_folders.push(item);
            }
        }
        "studio" => cfg.studio_pins.push(item),
        "games" => cfg.game_launchers.push(item),
        "taskbar" => {
            if !cfg.taskbar_pins.iter().any(|p| p.target.eq_ignore_ascii_case(&item.target) && !item.target.is_empty()) {
                cfg.taskbar_pins.push(item);
            }
        }
        custom_id => {
            if let Some(sp) = cfg.custom_spaces.iter_mut().find(|s| s.id == custom_id) {
                sp.items.push(item);
            }
        }
    }
}

pub fn remove_pin_from_space(cfg: &mut TestBarConfig, space: &str, item_id: &str) {
    let retain_fn = |list: &mut Vec<PinItem>| list.retain(|i| i.id != item_id);
    match space {
        "personal" => retain_fn(&mut cfg.personal_pins),
        "personal_links" => retain_fn(&mut cfg.personal_links),
        "work" => {
            retain_fn(&mut cfg.work_folders);
            retain_fn(&mut cfg.work_apps);
        }
        "studio" => retain_fn(&mut cfg.studio_pins),
        "games" => retain_fn(&mut cfg.game_launchers),
        "taskbar" => retain_fn(&mut cfg.taskbar_pins),
        custom_id => {
            if let Some(sp) = cfg.custom_spaces.iter_mut().find(|s| s.id == custom_id) {
                retain_fn(&mut sp.items);
            }
        }
    }
}

pub fn reorder_pins_in_space(cfg: &mut TestBarConfig, space: &str, from_idx: usize, to_idx: usize) {
    let list = match space {
        "personal" => Some(&mut cfg.personal_pins),
        "studio" => Some(&mut cfg.studio_pins),
        "work_folders" => Some(&mut cfg.work_folders),
        "taskbar" => Some(&mut cfg.taskbar_pins),
        custom_id => cfg
            .custom_spaces
            .iter_mut()
            .find(|s| s.id == custom_id)
            .map(|s| &mut s.items),
    };
    if let Some(vec) = list {
        if from_idx < vec.len() && to_idx < vec.len() && from_idx != to_idx {
            let item = vec.remove(from_idx);
            vec.insert(to_idx, item);
        }
    }
}

pub fn set_windows_autostart(enable: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_str = exe.to_string_lossy().to_string();
    let status = if enable {
        std::process::Command::new("reg")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "TestBar",
                "/t",
                "REG_SZ",
                "/d",
                &format!("\"{exe_str}\""),
                "/f",
            ])
            .status()
    } else {
        std::process::Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "TestBar",
                "/f",
            ])
            .status()
    };
    match status {
        Ok(s) if s.success() || !enable => Ok(()),
        Ok(s) => Err(format!("reg exit {s}")),
        Err(e) => Err(e.to_string()),
    }
}

pub fn ensure_parent_dir(p: &Path) -> Result<(), String> {
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrip_and_universal_pinning() {
        let mut cfg = TestBarConfig::default();
        assert_eq!(cfg.bar_mode, "dock");
        assert!(!cfg.personal_pins.is_empty());

        let folder_pin = pin(
            "custom-folder-1",
            "folder",
            "Проекты",
            "Папка",
            r"C:\Temp".to_string(),
            "Открыть в терминале",
            "56,189,248",
            "folder",
        );
        add_pin_to_space(&mut cfg, "personal", folder_pin);
        assert!(cfg.personal_pins.iter().any(|p| p.id == "custom-folder-1"));

        let prev_first = cfg.personal_pins[0].id.clone();
        reorder_pins_in_space(&mut cfg, "personal", 0, 1);
        assert_eq!(cfg.personal_pins[1].id, prev_first);

        remove_pin_from_space(&mut cfg, "personal", "custom-folder-1");
        assert!(!cfg.personal_pins.iter().any(|p| p.id == "custom-folder-1"));

        let json = serde_json::to_string(&cfg).expect("serialize");
        let parsed: TestBarConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.bar_mode, "dock");
    }
}
