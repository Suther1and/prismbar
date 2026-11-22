use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const WEATHER_CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const AUTO_GEO_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_HTTP_RESPONSE_BYTES: usize = 512 * 1024;
const HTTP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeatherCityOption {
    pub name: String,
    pub country: String,
    pub admin1: String,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HourlyForecast {
    pub time: String,
    pub temp_c: f64,
    pub condition_icon: String,
    pub precip_prob: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyForecast {
    pub date: String,
    pub label: String,
    pub temp_min_c: f64,
    pub temp_max_c: f64,
    pub condition_icon: String,
    pub precip_prob: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeatherSnapshot {
    pub available: bool,
    pub stale: bool,
    pub city: String,
    pub lat: f64,
    pub lon: f64,
    pub temp_c: f64,
    pub feels_like_c: f64,
    pub humidity: u8,
    pub wind_kmh: f64,
    pub condition_code: i32,
    pub condition_icon: String,
    pub condition_label: String,
    pub is_day: bool,
    pub hourly: Vec<HourlyForecast>,
    pub daily: Vec<DailyForecast>,
    pub updated_at_ms: u64,
    pub error: Option<String>,
}

impl WeatherSnapshot {
    fn unavailable(city: String, lat: f64, lon: f64, error: String) -> Self {
        Self {
            available: false,
            stale: false,
            city,
            lat,
            lon,
            temp_c: 0.0,
            feels_like_c: 0.0,
            humidity: 0,
            wind_kmh: 0.0,
            condition_code: 0,
            condition_icon: "cloud".to_string(),
            condition_label: "Нет данных".to_string(),
            is_day: true,
            hourly: Vec::new(),
            daily: Vec::new(),
            updated_at_ms: 0,
            error: Some(error),
        }
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn http_agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(4))
            .timeout_read(HTTP_TIMEOUT)
            .timeout(Duration::from_secs(6))
            .user_agent("PrismBar/3.3 (macOS)")
            .build()
    })
}

fn http_get_string(url: &str) -> Result<String, String> {
    let resp = http_agent()
        .get(url)
        .call()
        .map_err(|e| format!("HTTP error: {e}"))?;

    let mut body = Vec::with_capacity(8192);
    resp.into_reader()
        .take((MAX_HTTP_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut body)
        .map_err(|e| format!("HTTP read error: {e}"))?;

    if body.len() > MAX_HTTP_RESPONSE_BYTES {
        return Err("HTTP response exceeded 512 KB cap".to_string());
    }
    String::from_utf8(body).map_err(|e| format!("Invalid UTF-8 response: {e}"))
}

pub fn url_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 3);
    for &b in input.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                let hi = (b >> 4) & 0x0F;
                let lo = b & 0x0F;
                out.push(
                    char::from_digit(hi as u32, 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
                out.push(
                    char::from_digit(lo as u32, 16)
                        .unwrap_or('0')
                        .to_ascii_uppercase(),
                );
            }
        }
    }
    out
}

pub fn map_wmo_weather(code: i32, is_day: bool) -> (&'static str, &'static str) {
    match code {
        0 => (if is_day { "sun" } else { "moon" }, "Ясно"),
        1 => (
            if is_day {
                "partly-cloudy-day"
            } else {
                "partly-cloudy-night"
            },
            "Преимущественно ясно",
        ),
        2 => (
            if is_day {
                "partly-cloudy-day"
            } else {
                "partly-cloudy-night"
            },
            "Переменная облачность",
        ),
        3 => ("cloud", "Пасмурно"),
        45 | 48 => ("fog", "Туман"),
        51 | 53 | 55 => ("drizzle", "Морось"),
        56 | 57 => ("drizzle", "Ледяная морось"),
        61 => ("rain", "Небольшой дождь"),
        63 => ("rain", "Дождь"),
        65 => ("rain", "Сильный дождь"),
        66 | 67 => ("rain", "Ледяной дождь"),
        71 => ("snow", "Небольшой снег"),
        73 => ("snow", "Снег"),
        75 => ("snow", "Сильный снег"),
        77 => ("snow", "Снежная крупа"),
        80 | 81 => ("rain", "Ливень"),
        82 => ("rain", "Сильный ливень"),
        85 | 86 => ("snow", "Снегопад"),
        95 => ("storm", "Гроза"),
        96 | 99 => ("storm", "Гроза с градом"),
        _ => ("cloud", "Облачно"),
    }
}

pub fn format_russian_weekday_label(date_str: &str, is_today: bool) -> String {
    if is_today {
        return "Сегодня".to_string();
    }
    let parts: Vec<&str> = date_str.split('-').collect();
    if parts.len() != 3 {
        return date_str.to_string();
    }
    let Ok(year) = parts[0].parse::<i32>() else {
        return date_str.to_string();
    };
    let Ok(month) = parts[1].parse::<u32>() else {
        return date_str.to_string();
    };
    let Ok(day) = parts[2].parse::<u32>() else {
        return date_str.to_string();
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return date_str.to_string();
    }

    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe as i32 - 719_468;
    let wd = (days + 3).rem_euclid(7) as usize;
    const WEEKDAYS: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];
    WEEKDAYS[wd].to_string()
}

fn deserialize_opt_f64_or_string<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(f64),
        Str(String),
        Null,
    }
    match Option::<NumOrStr>::deserialize(deserializer)? {
        Some(NumOrStr::Num(v)) if v.is_finite() => Ok(Some(v)),
        Some(NumOrStr::Str(s)) => Ok(s.trim().parse::<f64>().ok().filter(|v| v.is_finite())),
        _ => Ok(None),
    }
}

#[derive(Debug, Deserialize)]
struct IpWhoIsResponse {
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_f64_or_string")]
    latitude: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_f64_or_string")]
    longitude: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct GeoJsResponse {
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    region: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_f64_or_string")]
    latitude: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_f64_or_string")]
    longitude: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct IpApiResponse {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default, rename = "regionName")]
    region_name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_f64_or_string")]
    lat: Option<f64>,
    #[serde(default, deserialize_with = "deserialize_opt_f64_or_string")]
    lon: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
struct DetectedLocation {
    city: String,
    lat: f64,
    lon: f64,
}

fn parse_ipwhois_json(json: &str) -> Result<DetectedLocation, String> {
    let parsed: IpWhoIsResponse =
        serde_json::from_str(json).map_err(|e| format!("ipwho.is parse error: {e}"))?;
    if parsed.success == Some(false) {
        return Err("ipwho.is returned success=false".to_string());
    }
    let lat = parsed
        .latitude
        .ok_or_else(|| "ipwho.is missing latitude".to_string())?;
    let lon = parsed
        .longitude
        .ok_or_else(|| "ipwho.is missing longitude".to_string())?;
    let city = parsed
        .city
        .or(parsed.region)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{lat:.2}, {lon:.2}"));
    Ok(DetectedLocation { city, lat, lon })
}

fn parse_geojs_json(json: &str) -> Result<DetectedLocation, String> {
    let parsed: GeoJsResponse =
        serde_json::from_str(json).map_err(|e| format!("geojs parse error: {e}"))?;
    let lat = parsed
        .latitude
        .ok_or_else(|| "geojs missing latitude".to_string())?;
    let lon = parsed
        .longitude
        .ok_or_else(|| "geojs missing longitude".to_string())?;
    let city = parsed
        .city
        .or(parsed.region)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{lat:.2}, {lon:.2}"));
    Ok(DetectedLocation { city, lat, lon })
}

fn parse_ip_api_json(json: &str) -> Result<DetectedLocation, String> {
    let parsed: IpApiResponse =
        serde_json::from_str(json).map_err(|e| format!("ip-api parse error: {e}"))?;
    if parsed
        .status
        .as_deref()
        .is_some_and(|s| s.eq_ignore_ascii_case("fail"))
    {
        return Err("ip-api returned status=fail".to_string());
    }
    let lat = parsed
        .lat
        .ok_or_else(|| "ip-api missing lat".to_string())?;
    let lon = parsed
        .lon
        .ok_or_else(|| "ip-api missing lon".to_string())?;
    let city = parsed
        .city
        .or(parsed.region_name)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("{lat:.2}, {lon:.2}"));
    Ok(DetectedLocation { city, lat, lon })
}

#[derive(Debug, Deserialize)]
struct GeocodingApiResponse {
    #[serde(default)]
    results: Vec<GeocodingApiEntry>,
}

#[derive(Debug, Deserialize)]
struct GeocodingApiEntry {
    #[serde(default)]
    name: String,
    #[serde(default)]
    country: String,
    #[serde(default)]
    admin1: String,
    latitude: f64,
    longitude: f64,
}

pub fn parse_geocoding_response(json: &str) -> Result<Vec<WeatherCityOption>, String> {
    let parsed: GeocodingApiResponse =
        serde_json::from_str(json).map_err(|e| format!("Geocoding parse error: {e}"))?;
    Ok(parsed
        .results
        .into_iter()
        .filter(|r| !r.name.trim().is_empty() && r.latitude.is_finite() && r.longitude.is_finite())
        .map(|r| WeatherCityOption {
            name: r.name.trim().to_string(),
            country: r.country.trim().to_string(),
            admin1: r.admin1.trim().to_string(),
            lat: r.latitude,
            lon: r.longitude,
        })
        .collect())
}

#[derive(Debug, Deserialize)]
struct OpenMeteoForecastResponse {
    current: OpenMeteoCurrent,
    #[serde(default)]
    hourly: OpenMeteoHourly,
    #[serde(default)]
    daily: OpenMeteoDaily,
}

#[derive(Debug, Deserialize)]
struct OpenMeteoCurrent {
    #[serde(default)]
    time: String,
    temperature_2m: f64,
    #[serde(default)]
    apparent_temperature: Option<f64>,
    #[serde(default)]
    relative_humidity_2m: Option<f64>,
    #[serde(default)]
    wind_speed_10m: Option<f64>,
    #[serde(default)]
    weather_code: i32,
    #[serde(default = "default_is_day_num")]
    is_day: i32,
}

fn default_is_day_num() -> i32 {
    1
}

#[derive(Debug, Default, Deserialize)]
struct OpenMeteoHourly {
    #[serde(default)]
    time: Vec<String>,
    #[serde(default)]
    temperature_2m: Vec<Option<f64>>,
    #[serde(default)]
    weather_code: Vec<Option<i32>>,
    #[serde(default)]
    precipitation_probability: Vec<Option<f64>>,
}

#[derive(Debug, Default, Deserialize)]
struct OpenMeteoDaily {
    #[serde(default)]
    time: Vec<String>,
    #[serde(default)]
    weather_code: Vec<Option<i32>>,
    #[serde(default)]
    temperature_2m_max: Vec<Option<f64>>,
    #[serde(default)]
    temperature_2m_min: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_probability_max: Vec<Option<f64>>,
}

fn round_temp_1(val: f64) -> f64 {
    if !val.is_finite() {
        return 0.0;
    }
    (val * 10.0).round() / 10.0
}

fn hour_prefix_13(iso_time: &str) -> Option<&str> {
    let s = iso_time.trim();
    if s.len() >= 13 {
        Some(&s[..13])
    } else {
        None
    }
}

fn format_hh_mm(iso_time: &str) -> (String, bool) {
    let hh_mm = iso_time
        .split('T')
        .nth(1)
        .unwrap_or(iso_time)
        .trim();
    let hour_num = hh_mm
        .split(':')
        .next()
        .and_then(|h| h.parse::<u32>().ok())
        .unwrap_or(12);
    let is_day = (6..21).contains(&hour_num);
    let label = if hh_mm.len() >= 5 {
        hh_mm[..5].to_string()
    } else {
        format!("{hour_num:02}:00")
    };
    (label, is_day)
}

pub fn parse_open_meteo_forecast(
    json: &str,
    city: &str,
    lat: f64,
    lon: f64,
    updated_at_ms: u64,
) -> Result<WeatherSnapshot, String> {
    let parsed: OpenMeteoForecastResponse =
        serde_json::from_str(json).map_err(|e| format!("Forecast parse error: {e}"))?;

    let is_day = parsed.current.is_day != 0;
    let code = parsed.current.weather_code;
    let (icon, label) = map_wmo_weather(code, is_day);

    let current_hour_key = hour_prefix_13(&parsed.current.time);
    let hourly_len = parsed
        .hourly
        .time
        .len()
        .min(parsed.hourly.temperature_2m.len())
        .min(parsed.hourly.weather_code.len());

    let start_idx = current_hour_key
        .and_then(|cur| {
            parsed.hourly.time[..hourly_len]
                .iter()
                .position(|t| hour_prefix_13(t).is_some_and(|h| h >= cur))
        })
        .unwrap_or(0);

    let mut hourly = Vec::with_capacity(12);
    for i in start_idx..hourly_len {
        if hourly.len() >= 12 {
            break;
        }
        let raw_time = &parsed.hourly.time[i];
        let temp = parsed.hourly.temperature_2m[i].unwrap_or(parsed.current.temperature_2m);
        let h_code = parsed.hourly.weather_code[i].unwrap_or(code);
        let precip = parsed
            .hourly
            .precipitation_probability
            .get(i)
            .copied()
            .flatten()
            .unwrap_or(0.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        let (hh_label, h_is_day) = format_hh_mm(raw_time);
        let (h_icon, _) = map_wmo_weather(h_code, h_is_day);
        hourly.push(HourlyForecast {
            time: hh_label,
            temp_c: round_temp_1(temp),
            condition_icon: h_icon.to_string(),
            precip_prob: precip,
        });
    }

    let daily_len = parsed
        .daily
        .time
        .len()
        .min(parsed.daily.weather_code.len())
        .min(parsed.daily.temperature_2m_max.len())
        .min(parsed.daily.temperature_2m_min.len());
    let current_date = parsed.current.time.split('T').next().unwrap_or("");
    let mut daily = Vec::with_capacity(daily_len.min(5));
    for i in 0..daily_len.min(5) {
        let date = parsed.daily.time[i].trim().to_string();
        let d_code = parsed.daily.weather_code[i].unwrap_or(code);
        let t_max = parsed.daily.temperature_2m_max[i].unwrap_or(parsed.current.temperature_2m);
        let t_min = parsed.daily.temperature_2m_min[i].unwrap_or(t_max);
        let precip = parsed
            .daily
            .precipitation_probability_max
            .get(i)
            .copied()
            .flatten()
            .unwrap_or(0.0)
            .round()
            .clamp(0.0, 100.0) as u8;
        let (d_icon, _) = map_wmo_weather(d_code, true);
        let is_today = i == 0 || (!current_date.is_empty() && date == current_date);
        daily.push(DailyForecast {
            label: format_russian_weekday_label(&date, is_today),
            date,
            temp_min_c: round_temp_1(t_min),
            temp_max_c: round_temp_1(t_max),
            condition_icon: d_icon.to_string(),
            precip_prob: precip,
        });
    }

    Ok(WeatherSnapshot {
        available: true,
        stale: false,
        city: city.trim().to_string(),
        lat,
        lon,
        temp_c: round_temp_1(parsed.current.temperature_2m),
        feels_like_c: round_temp_1(
            parsed
                .current
                .apparent_temperature
                .unwrap_or(parsed.current.temperature_2m),
        ),
        humidity: parsed
            .current
            .relative_humidity_2m
            .unwrap_or(0.0)
            .round()
            .clamp(0.0, 100.0) as u8,
        wind_kmh: round_temp_1(parsed.current.wind_speed_10m.unwrap_or(0.0).max(0.0)),
        condition_code: code,
        condition_icon: icon.to_string(),
        condition_label: label.to_string(),
        is_day,
        hourly,
        daily,
        updated_at_ms,
        error: None,
    })
}

#[derive(Clone)]
struct CachedWeatherEntry {
    fetched_at: Instant,
    snapshot: WeatherSnapshot,
}

#[derive(Clone)]
struct CachedAutoLocation {
    fetched_at: Instant,
    loc: DetectedLocation,
}

static WEATHER_CACHE: OnceLock<Mutex<HashMap<String, CachedWeatherEntry>>> = OnceLock::new();
static AUTO_LOCATION_CACHE: OnceLock<Mutex<Option<CachedAutoLocation>>> = OnceLock::new();
static WEATHER_FETCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn normalize_location_key(city: Option<&str>, lat: Option<f64>, lon: Option<f64>) -> String {
    let clean_city = city.map(str::trim).filter(|s| !s.is_empty());
    match (lat.filter(|v| v.is_finite()), lon.filter(|v| v.is_finite())) {
        (Some(la), Some(lo)) => {
            let label = clean_city.unwrap_or("").to_lowercase();
            format!("coords:{la:.3},{lo:.3}:{label}")
        }
        _ => match clean_city {
            Some(c) => format!("city:{}", c.to_lowercase()),
            None => "auto".to_string(),
        },
    }
}

fn evaluate_cache_entry(
    entry: Option<&CachedWeatherEntry>,
    now: Instant,
    force: bool,
    ttl: Duration,
) -> Option<WeatherSnapshot> {
    if force {
        return None;
    }
    let e = entry?;
    if now.saturating_duration_since(e.fetched_at) < ttl {
        Some(e.snapshot.clone())
    } else {
        None
    }
}

fn resolve_auto_location() -> Result<DetectedLocation, String> {
    let cache = AUTO_LOCATION_CACHE.get_or_init(|| Mutex::new(None));
    let cached_fallback = {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.as_ref() {
            if entry.fetched_at.elapsed() < AUTO_GEO_CACHE_TTL {
                return Ok(entry.loc.clone());
            }
            Some(entry.loc.clone())
        } else {
            None
        }
    };

    let resolved = http_get_string("https://get.geojs.io/v1/ip/geo.json")
        .and_then(|body| parse_geojs_json(&body))
        .or_else(|err1| {
            http_get_string("https://ipwho.is/")
                .and_then(|body| parse_ipwhois_json(&body))
                .map_err(|err2| format!("{err1}; {err2}"))
        })
        .or_else(|err12| {
            http_get_string("http://ip-api.com/json/?lang=ru")
                .and_then(|body| parse_ip_api_json(&body))
                .map_err(|err3| format!("Geo lookup failed ({err12}; {err3})"))
        });

    match resolved {
        Ok(loc) => {
            let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(CachedAutoLocation {
                fetched_at: Instant::now(),
                loc: loc.clone(),
            });
            Ok(loc)
        }
        Err(err) => cached_fallback.ok_or(err),
    }
}

/// Searches matching cities via Open-Meteo geocoding API.
pub fn search_weather_cities(query: &str) -> Result<Vec<WeatherCityOption>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=6&language=ru&format=json",
        url_encode(trimmed)
    );
    let body = http_get_string(&url)?;
    parse_geocoding_response(&body)
}

fn fetch_live_weather(
    city: Option<&str>,
    lat: Option<f64>,
    lon: Option<f64>,
) -> Result<WeatherSnapshot, String> {
    let clean_city = city.map(str::trim).filter(|s| !s.is_empty());
    let (resolved_city, resolved_lat, resolved_lon) = match (
        lat.filter(|v| v.is_finite()),
        lon.filter(|v| v.is_finite()),
    ) {
        (Some(la), Some(lo)) => {
            let name = clean_city
                .map(str::to_string)
                .unwrap_or_else(|| format!("{la:.2}, {lo:.2}"));
            (name, la, lo)
        }
        _ => match clean_city {
            Some(c) => {
                let options = search_weather_cities(c)?;
                let first = options
                    .into_iter()
                    .next()
                    .ok_or_else(|| format!("Город «{c}» не найден"))?;
                (first.name, first.lat, first.lon)
            }
            None => {
                let auto = resolve_auto_location()?;
                (auto.city, auto.lat, auto.lon)
            }
        },
    };

    let url = format!(
        "https://api.open-meteo.com/v1/forecast?latitude={resolved_lat:.4}&longitude={resolved_lon:.4}&current=temperature_2m,apparent_temperature,relative_humidity_2m,wind_speed_10m,weather_code,is_day&hourly=temperature_2m,weather_code,precipitation_probability&daily=weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max&timezone=auto&forecast_days=5"
    );
    let body = http_get_string(&url)?;
    parse_open_meteo_forecast(&body, &resolved_city, resolved_lat, resolved_lon, now_unix_ms())
}

/// Fetches Open-Meteo weather with 15-minute TTL cache and stale-on-error fallback.
pub fn get_weather(
    city: Option<&str>,
    lat: Option<f64>,
    lon: Option<f64>,
    force: bool,
) -> WeatherSnapshot {
    let key = normalize_location_key(city, lat, lon);
    let cache = WEATHER_CACHE.get_or_init(|| Mutex::new(HashMap::new()));

    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = evaluate_cache_entry(guard.get(&key), Instant::now(), force, WEATHER_CACHE_TTL) {
            return hit;
        }
    }

    let fetch_lock = WEATHER_FETCH_LOCK.get_or_init(|| Mutex::new(()));
    let _fetch_guard = fetch_lock.lock().unwrap_or_else(|e| e.into_inner());

    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(hit) = evaluate_cache_entry(guard.get(&key), Instant::now(), force, WEATHER_CACHE_TTL) {
            return hit;
        }
    }

    match fetch_live_weather(city, lat, lon) {
        Ok(snapshot) => {
            let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            if guard.len() > 24 {
                guard.clear();
            }
            guard.insert(
                key,
                CachedWeatherEntry {
                    fetched_at: Instant::now(),
                    snapshot: snapshot.clone(),
                },
            );
            snapshot
        }
        Err(err) => {
            let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(cached) = guard.get(&key) {
                let mut stale = cached.snapshot.clone();
                stale.stale = true;
                stale.error = Some(err);
                return stale;
            }
            WeatherSnapshot::unavailable(
                city.unwrap_or("").trim().to_string(),
                lat.unwrap_or(0.0),
                lon.unwrap_or(0.0),
                err,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_wmo_codes_and_russian_weekdays() {
        assert_eq!(map_wmo_weather(0, true), ("sun", "Ясно"));
        assert_eq!(map_wmo_weather(0, false), ("moon", "Ясно"));
        assert_eq!(map_wmo_weather(63, true), ("rain", "Дождь"));
        assert_eq!(map_wmo_weather(95, true), ("storm", "Гроза"));

        assert_eq!(format_russian_weekday_label("2026-11-21", true), "Сегодня");
        assert_eq!(format_russian_weekday_label("2026-11-23", false), "Пн");
    }

    #[test]
    fn parses_geo_and_forecast_payloads() {
        let geo = parse_geojs_json(r#"{"city":"Almaty","latitude":"43.2389","longitude":"76.8897"}"#).unwrap();
        assert_eq!(geo.city, "Almaty");
        assert!((geo.lat - 43.2389).abs() < 0.001);

        let ip_api = parse_ip_api_json(r#"{"status":"success","city":"Москва","lat":55.75,"lon":37.61}"#).unwrap();
        assert_eq!(ip_api.city, "Москва");

        let forecast_json = r#"{
            "current":{"time":"2026-11-22T12:00","temperature_2m":11.4,"apparent_temperature":9.8,"relative_humidity_2m":60,"wind_speed_10m":14.2,"weather_code":2,"is_day":1},
            "hourly":{"time":["2026-11-22T12:00","2026-11-22T13:00"],"temperature_2m":[11.4,12.1],"weather_code":[2,0],"precipitation_probability":[10,0]},
            "daily":{"time":["2026-11-22","2026-11-23"],"weather_code":[2,61],"temperature_2m_max":[13.0,9.0],"temperature_2m_min":[5.0,3.0],"precipitation_probability_max":[20,70]}
        }"#;
        let snap = parse_open_meteo_forecast(forecast_json, "Москва", 55.75, 37.61, 12345).unwrap();
        assert!(snap.available);
        assert_eq!(snap.city, "Москва");
        assert_eq!(snap.hourly.len(), 2);
        assert_eq!(snap.daily.len(), 2);
        assert_eq!(snap.daily[0].label, "Сегодня");
    }
}
