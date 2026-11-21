use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;
use std::ffi::{c_void, CString};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::core::{PCSTR, PCWSTR};
use windows::Win32::Foundation::{GetLastError, HMODULE};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

const WEATHER_CACHE_TTL: Duration = Duration::from_secs(15 * 60);
const AUTO_GEO_CACHE_TTL: Duration = Duration::from_secs(60 * 60);
const MAX_HTTP_RESPONSE_BYTES: usize = 512 * 1024;
const HTTP_CONNECT_TIMEOUT_MS: i32 = 4000;
const HTTP_SEND_TIMEOUT_MS: i32 = 4000;
const HTTP_RECEIVE_TIMEOUT_MS: i32 = 6000;

const WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY: u32 = 4;
const WINHTTP_FLAG_SECURE: u32 = 0x0080_0000;
const WINHTTP_QUERY_STATUS_CODE: u32 = 19;
const WINHTTP_QUERY_FLAG_NUMBER: u32 = 0x2000_0000;
const INTERNET_DEFAULT_HTTPS_PORT: u16 = 443;

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

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ======================= DYNAMIC WINHTTP CLIENT =======================

type HINTERNET = *mut c_void;
type FnWinHttpOpen = unsafe extern "system" fn(
    psz_agent: *const u16,
    dw_access_type: u32,
    psz_proxy: *const u16,
    psz_proxy_bypass: *const u16,
    dw_flags: u32,
) -> HINTERNET;
type FnWinHttpSetTimeouts = unsafe extern "system" fn(
    h_internet: HINTERNET,
    n_resolve_timeout: i32,
    n_connect_timeout: i32,
    n_send_timeout: i32,
    n_receive_timeout: i32,
) -> i32;
type FnWinHttpConnect = unsafe extern "system" fn(
    h_session: HINTERNET,
    pswz_server_name: *const u16,
    n_server_port: u16,
    dw_reserved: u32,
) -> HINTERNET;
type FnWinHttpOpenRequest = unsafe extern "system" fn(
    h_connect: HINTERNET,
    pwsz_verb: *const u16,
    pwsz_object_name: *const u16,
    pwsz_version: *const u16,
    pwsz_referrer: *const u16,
    ppwsz_accept_types: *const *const u16,
    dw_flags: u32,
) -> HINTERNET;
type FnWinHttpSendRequest = unsafe extern "system" fn(
    h_request: HINTERNET,
    lpsz_headers: *const u16,
    dw_headers_length: u32,
    lp_optional: *const c_void,
    dw_optional_length: u32,
    dw_total_length: u32,
    dw_context: usize,
) -> i32;
type FnWinHttpReceiveResponse =
    unsafe extern "system" fn(h_request: HINTERNET, lp_reserved: *mut c_void) -> i32;
type FnWinHttpQueryHeaders = unsafe extern "system" fn(
    h_request: HINTERNET,
    dw_info_level: u32,
    pwsz_name: *const u16,
    lp_buffer: *mut c_void,
    lpdw_buffer_length: *mut u32,
    lpdw_index: *mut u32,
) -> i32;
type FnWinHttpQueryDataAvailable = unsafe extern "system" fn(
    h_request: HINTERNET,
    lpdw_number_of_bytes_available: *mut u32,
) -> i32;
type FnWinHttpReadData = unsafe extern "system" fn(
    h_request: HINTERNET,
    lp_buffer: *mut c_void,
    dw_number_of_bytes_to_read: u32,
    lpdw_number_of_bytes_read: *mut u32,
) -> i32;
type FnWinHttpCloseHandle = unsafe extern "system" fn(h_internet: HINTERNET) -> i32;

struct WinHttpApi {
    _hmod: HMODULE,
    open: FnWinHttpOpen,
    set_timeouts: FnWinHttpSetTimeouts,
    connect: FnWinHttpConnect,
    open_request: FnWinHttpOpenRequest,
    send_request: FnWinHttpSendRequest,
    receive_response: FnWinHttpReceiveResponse,
    query_headers: FnWinHttpQueryHeaders,
    query_data_available: FnWinHttpQueryDataAvailable,
    read_data: FnWinHttpReadData,
    close_handle: FnWinHttpCloseHandle,
}

unsafe impl Send for WinHttpApi {}
unsafe impl Sync for WinHttpApi {}

static WINHTTP_API: OnceLock<Result<WinHttpApi, String>> = OnceLock::new();

struct WinHttpHandle {
    handle: HINTERNET,
    close_fn: FnWinHttpCloseHandle,
}

impl WinHttpHandle {
    fn new(handle: HINTERNET, close_fn: FnWinHttpCloseHandle) -> Option<Self> {
        if handle.is_null() {
            None
        } else {
            Some(Self { handle, close_fn })
        }
    }
}

impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe {
                let _ = (self.close_fn)(self.handle);
            }
        }
    }
}

fn load_winhttp() -> Result<&'static WinHttpApi, String> {
    let res = WINHTTP_API.get_or_init(|| unsafe {
        let dll = to_wide("winhttp.dll");
        let hmod = LoadLibraryW(PCWSTR(dll.as_ptr()))
            .map_err(|e| format!("LoadLibraryW(winhttp.dll) failed: {e}"))?;
        if hmod.is_invalid() {
            return Err("LoadLibraryW(winhttp.dll) returned invalid HMODULE".to_string());
        }

        let get_fn = |name: &str| -> Result<*const (), String> {
            let cs = CString::new(name).map_err(|_| format!("Invalid symbol {name}"))?;
            GetProcAddress(hmod, PCSTR(cs.as_ptr() as *const u8))
                .map(|p| p as *const ())
                .ok_or_else(|| format!("GetProcAddress({name}) missing in winhttp.dll"))
        };

        Ok(WinHttpApi {
            _hmod: hmod,
            open: std::mem::transmute(get_fn("WinHttpOpen")?),
            set_timeouts: std::mem::transmute(get_fn("WinHttpSetTimeouts")?),
            connect: std::mem::transmute(get_fn("WinHttpConnect")?),
            open_request: std::mem::transmute(get_fn("WinHttpOpenRequest")?),
            send_request: std::mem::transmute(get_fn("WinHttpSendRequest")?),
            receive_response: std::mem::transmute(get_fn("WinHttpReceiveResponse")?),
            query_headers: std::mem::transmute(get_fn("WinHttpQueryHeaders")?),
            query_data_available: std::mem::transmute(get_fn("WinHttpQueryDataAvailable")?),
            read_data: std::mem::transmute(get_fn("WinHttpReadData")?),
            close_handle: std::mem::transmute(get_fn("WinHttpCloseHandle")?),
        })
    });

    res.as_ref().map_err(Clone::clone)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedHttpsUrl<'a> {
    host: &'a str,
    port: u16,
    path_and_query: &'a str,
}

fn parse_https_url(url: &str) -> Result<ParsedHttpsUrl<'_>, String> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| format!("Expected https:// URL, got: {url}"))?;
    let (authority, path_and_query) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(format!("Missing host in URL: {url}"));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !h.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            let parsed_port = p
                .parse::<u16>()
                .map_err(|_| format!("Invalid port in URL: {url}"))?;
            (h, parsed_port)
        }
        _ => (authority, INTERNET_DEFAULT_HTTPS_PORT),
    };
    Ok(ParsedHttpsUrl {
        host,
        port,
        path_and_query,
    })
}

fn winhttp_last_error(step: &str) -> String {
    let code = unsafe { GetLastError().0 };
    format!("{step} failed (Win32 error {code})")
}

fn http_get_string(url: &str) -> Result<String, String> {
    let api = load_winhttp()?;
    let parsed = parse_https_url(url)?;

    let agent_w = to_wide("TestBar/1.0");
    let session = unsafe {
        WinHttpHandle::new(
            (api.open)(
                agent_w.as_ptr(),
                WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                std::ptr::null(),
                std::ptr::null(),
                0,
            ),
            api.close_handle,
        )
    }
    .ok_or_else(|| winhttp_last_error("WinHttpOpen"))?;

    unsafe {
        if (api.set_timeouts)(
            session.handle,
            HTTP_CONNECT_TIMEOUT_MS,
            HTTP_CONNECT_TIMEOUT_MS,
            HTTP_SEND_TIMEOUT_MS,
            HTTP_RECEIVE_TIMEOUT_MS,
        ) == 0
        {
            return Err(winhttp_last_error("WinHttpSetTimeouts"));
        }
    }

    let host_w = to_wide(parsed.host);
    let connect = unsafe {
        WinHttpHandle::new(
            (api.connect)(session.handle, host_w.as_ptr(), parsed.port, 0),
            api.close_handle,
        )
    }
    .ok_or_else(|| winhttp_last_error("WinHttpConnect"))?;

    let verb_w = to_wide("GET");
    let path_w = to_wide(parsed.path_and_query);
    let request = unsafe {
        WinHttpHandle::new(
            (api.open_request)(
                connect.handle,
                verb_w.as_ptr(),
                path_w.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                WINHTTP_FLAG_SECURE,
            ),
            api.close_handle,
        )
    }
    .ok_or_else(|| winhttp_last_error("WinHttpOpenRequest"))?;

    unsafe {
        if (api.send_request)(
            request.handle,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            0,
            0,
        ) == 0
        {
            return Err(winhttp_last_error("WinHttpSendRequest"));
        }

        if (api.receive_response)(request.handle, std::ptr::null_mut()) == 0 {
            return Err(winhttp_last_error("WinHttpReceiveResponse"));
        }

        let mut status_code: u32 = 0;
        let mut status_size: u32 = std::mem::size_of::<u32>() as u32;
        if (api.query_headers)(
            request.handle,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            std::ptr::null(),
            &mut status_code as *mut u32 as *mut c_void,
            &mut status_size,
            std::ptr::null_mut(),
        ) != 0
            && !(200..300).contains(&status_code)
        {
            return Err(format!("HTTP error {status_code}"));
        }

        let mut body = Vec::with_capacity(8192);
        loop {
            let mut avail: u32 = 0;
            if (api.query_data_available)(request.handle, &mut avail) == 0 {
                return Err(winhttp_last_error("WinHttpQueryDataAvailable"));
            }
            if avail == 0 {
                break;
            }
            let remaining = MAX_HTTP_RESPONSE_BYTES.saturating_sub(body.len());
            if remaining == 0 {
                return Err("HTTP response exceeded 512 KB cap".to_string());
            }
            let to_read = (avail as usize).min(remaining).min(32 * 1024);
            let mut chunk = vec![0u8; to_read];
            let mut bytes_read: u32 = 0;
            if (api.read_data)(
                request.handle,
                chunk.as_mut_ptr() as *mut c_void,
                to_read as u32,
                &mut bytes_read,
            ) == 0
            {
                return Err(winhttp_last_error("WinHttpReadData"));
            }
            if bytes_read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..bytes_read as usize]);
        }

        String::from_utf8(body).map_err(|e| format!("Invalid UTF-8 response: {e}"))
    }
}

// ======================= PURE HELPERS: URL, WMO, CALENDAR, PARSERS =======================

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
                out.push(char::from_digit(hi as u32, 16).unwrap_or('0').to_ascii_uppercase());
                out.push(char::from_digit(lo as u32, 16).unwrap_or('0').to_ascii_uppercase());
            }
        }
    }
    out
}

pub fn map_wmo_weather(code: i32, is_day: bool) -> (&'static str, &'static str) {
    match code {
        0 => (
            if is_day { "sun" } else { "moon" },
            "Ясно",
        ),
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

/// Howard Hinnant's civil-to-days formula mapping `"YYYY-MM-DD"` to Russian short weekday label.
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
    // 1970-01-01 was Thursday (index 3 in Mon=0..Sun=6)
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

// ======================= CACHE & PUBLIC API =======================

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

    let primary = http_get_string("https://ipwho.is/").and_then(|body| parse_ipwhois_json(&body));
    let resolved = match primary {
        Ok(loc) => Ok(loc),
        Err(err1) => match http_get_string("https://get.geojs.io/v1/ip/geo.json")
            .and_then(|body| parse_geojs_json(&body))
        {
            Ok(loc) => Ok(loc),
            Err(err2) => Err(format!("Geo lookup failed ({err1}; {err2})")),
        },
    };

    match resolved {
        Ok(loc) => {
            let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
            *guard = Some(CachedAutoLocation {
                fetched_at: Instant::now(),
                loc: loc.clone(),
            });
            Ok(loc)
        }
        Err(err) => {
            if let Some(stale_loc) = cached_fallback {
                Ok(stale_loc)
            } else {
                Err(err)
            }
        }
    }
}

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

    // Double-checked cache lookup after acquiring single-flight lock
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
    fn url_encode_handles_cyrillic_and_spaces() {
        assert_eq!(url_encode("Москва"), "%D0%9C%D0%BE%D1%81%D0%BA%D0%B2%D0%B0");
        assert_eq!(url_encode("New York-1_2.3~"), "New%20York-1_2.3~");
    }

    #[test]
    fn parse_https_url_handles_host_port_and_query() {
        assert_eq!(
            parse_https_url("https://api.open-meteo.com/v1/forecast?lat=55.7").unwrap(),
            ParsedHttpsUrl {
                host: "api.open-meteo.com",
                port: 443,
                path_and_query: "/v1/forecast?lat=55.7",
            }
        );
        assert_eq!(
            parse_https_url("https://ipwho.is").unwrap(),
            ParsedHttpsUrl {
                host: "ipwho.is",
                port: 443,
                path_and_query: "/",
            }
        );
        assert!(parse_https_url("http://insecure.example").is_err());
    }

    #[test]
    fn wmo_mapping_covers_day_night_and_all_icon_keys() {
        assert_eq!(map_wmo_weather(0, true), ("sun", "Ясно"));
        assert_eq!(map_wmo_weather(0, false), ("moon", "Ясно"));
        assert_eq!(
            map_wmo_weather(2, true),
            ("partly-cloudy-day", "Переменная облачность")
        );
        assert_eq!(
            map_wmo_weather(2, false),
            ("partly-cloudy-night", "Переменная облачность")
        );
        assert_eq!(map_wmo_weather(3, true).0, "cloud");
        assert_eq!(map_wmo_weather(45, true).0, "fog");
        assert_eq!(map_wmo_weather(53, true).0, "drizzle");
        assert_eq!(map_wmo_weather(63, true).0, "rain");
        assert_eq!(map_wmo_weather(73, true).0, "snow");
        assert_eq!(map_wmo_weather(95, true).0, "storm");
        assert_eq!(map_wmo_weather(999, true).0, "cloud");
    }

    #[test]
    fn russian_weekday_label_formatting_matches_calendar() {
        assert_eq!(format_russian_weekday_label("2026-11-21", true), "Сегодня");
        // 2026-11-21 is Saturday, 2026-11-22 is Sunday, 2026-11-23 is Monday
        assert_eq!(format_russian_weekday_label("2026-11-21", false), "Сб");
        assert_eq!(format_russian_weekday_label("2026-11-22", false), "Вс");
        assert_eq!(format_russian_weekday_label("2026-11-23", false), "Пн");
        assert_eq!(format_russian_weekday_label("2026-11-24", false), "Вт");
        assert_eq!(format_russian_weekday_label("2026-11-25", false), "Ср");
        assert_eq!(format_russian_weekday_label("2026-11-26", false), "Чт");
        assert_eq!(format_russian_weekday_label("2026-11-27", false), "Пт");
        // Leap day check: 2024-02-29 was Thursday
        assert_eq!(format_russian_weekday_label("2024-02-29", false), "Чт");
    }

    #[test]
    fn open_meteo_json_parses_current_hourly_next_12h_and_daily() {
        let mut hourly_times = Vec::new();
        let mut hourly_temps = Vec::new();
        let mut hourly_codes = Vec::new();
        let mut hourly_precip = Vec::new();
        for h in 0..24 {
            hourly_times.push(format!("\"2026-11-21T{h:02}:00\""));
            hourly_temps.push(format!("{:.1}", (h as f64) * 0.5));
            hourly_codes.push(if h < 18 { "2" } else { "61" });
            hourly_precip.push(if h == 15 { "null".to_string() } else { format!("{}", h * 3) });
        }
        for h in 0..10 {
            hourly_times.push(format!("\"2026-11-22T{h:02}:00\""));
            hourly_temps.push(format!("{:.1}", -1.0 + h as f64));
            hourly_codes.push("0");
            hourly_precip.push("5".to_string());
        }

        let sample_json = format!(
            r#"{{
                "current": {{
                    "time": "2026-11-21T19:45",
                    "temperature_2m": 3.44,
                    "apparent_temperature": -0.82,
                    "relative_humidity_2m": 82,
                    "wind_speed_10m": 14.26,
                    "weather_code": 61,
                    "is_day": 0
                }},
                "hourly": {{
                    "time": [{}],
                    "temperature_2m": [{}],
                    "weather_code": [{}],
                    "precipitation_probability": [{}]
                }},
                "daily": {{
                    "time": ["2026-11-21", "2026-11-22", "2026-11-23"],
                    "weather_code": [61, 0, 73],
                    "temperature_2m_max": [5.14, 4.0, -1.2],
                    "temperature_2m_min": [0.22, -2.5, -5.8],
                    "precipitation_probability_max": [75, null, 40]
                }}
            }}"#,
            hourly_times.join(","),
            hourly_temps.join(","),
            hourly_codes.join(","),
            hourly_precip.join(","),
        );

        let snap = parse_open_meteo_forecast(&sample_json, "Москва", 55.75, 37.62, 1_700_000_000_000)
            .expect("should parse valid Open-Meteo response");

        assert!(snap.available);
        assert!(!snap.stale);
        assert_eq!(snap.city, "Москва");
        assert_eq!(snap.temp_c, 3.4);
        assert_eq!(snap.feels_like_c, -0.8);
        assert_eq!(snap.humidity, 82);
        assert_eq!(snap.wind_kmh, 14.3);
        assert_eq!(snap.condition_code, 61);
        assert_eq!(snap.condition_icon, "rain");
        assert_eq!(snap.condition_label, "Небольшой дождь");
        assert!(!snap.is_day);

        // Next 12 hours starting from current hour 19:00 through 06:00 next day
        assert_eq!(snap.hourly.len(), 12);
        assert_eq!(snap.hourly[0].time, "19:00");
        assert_eq!(snap.hourly[0].temp_c, 9.5);
        assert_eq!(snap.hourly[0].condition_icon, "rain");
        assert_eq!(snap.hourly[0].precip_prob, 57);
        assert_eq!(snap.hourly[5].time, "00:00");
        assert_eq!(snap.hourly[5].condition_icon, "moon");
        assert_eq!(snap.hourly[11].time, "06:00");
        assert_eq!(snap.hourly[11].condition_icon, "sun");

        assert_eq!(snap.daily.len(), 3);
        assert_eq!(snap.daily[0].label, "Сегодня");
        assert_eq!(snap.daily[0].temp_max_c, 5.1);
        assert_eq!(snap.daily[0].precip_prob, 75);
        assert_eq!(snap.daily[1].label, "Вс");
        assert_eq!(snap.daily[1].condition_icon, "sun");
        assert_eq!(snap.daily[1].precip_prob, 0);
        assert_eq!(snap.daily[2].label, "Пн");
        assert_eq!(snap.daily[2].condition_icon, "snow");
    }

    #[test]
    fn geo_and_city_search_json_parsers_handle_string_coords_and_missing_fields() {
        let ipwho = r#"{"success":true,"city":"Санкт-Петербург","latitude":59.9343,"longitude":30.3351}"#;
        let loc1 = parse_ipwhois_json(ipwho).unwrap();
        assert_eq!(loc1.city, "Санкт-Петербург");
        assert!((loc1.lat - 59.9343).abs() < 1e-6);

        let geojs = r#"{"city":"Kazan","latitude":"55.7961","longitude":"49.1064"}"#;
        let loc2 = parse_geojs_json(geojs).unwrap();
        assert_eq!(loc2.city, "Kazan");
        assert!((loc2.lat - 55.7961).abs() < 1e-6);

        let search_empty = r#"{}"#;
        assert!(parse_geocoding_response(search_empty).unwrap().is_empty());

        let search_ok = r#"{"results":[{"name":"Москва","country":"Россия","admin1":"Москва","latitude":55.7522,"longitude":37.6156}]}"#;
        let cities = parse_geocoding_response(search_ok).unwrap();
        assert_eq!(cities.len(), 1);
        assert_eq!(cities[0].name, "Москва");
    }

    #[test]
    fn cache_hit_ttl_and_force_logic_works_deterministically() {
        let base = Instant::now();
        let snap = WeatherSnapshot {
            available: true,
            stale: false,
            city: "Москва".to_string(),
            lat: 55.75,
            lon: 37.62,
            temp_c: 5.0,
            feels_like_c: 2.0,
            humidity: 70,
            wind_kmh: 10.0,
            condition_code: 0,
            condition_icon: "sun".to_string(),
            condition_label: "Ясно".to_string(),
            is_day: true,
            hourly: Vec::new(),
            daily: Vec::new(),
            updated_at_ms: 12345,
            error: None,
        };
        let entry = CachedWeatherEntry {
            fetched_at: base,
            snapshot: snap.clone(),
        };

        // Within 15m TTL & force=false -> cache hit
        let hit = evaluate_cache_entry(
            Some(&entry),
            base + Duration::from_secs(10 * 60),
            false,
            WEATHER_CACHE_TTL,
        );
        assert_eq!(hit, Some(snap));

        // force=true -> bypass cache
        assert_eq!(
            evaluate_cache_entry(
                Some(&entry),
                base + Duration::from_secs(60),
                true,
                WEATHER_CACHE_TTL
            ),
            None
        );

        // Expired past 15m -> cache miss
        assert_eq!(
            evaluate_cache_entry(
                Some(&entry),
                base + Duration::from_secs(15 * 60 + 1),
                false,
                WEATHER_CACHE_TTL
            ),
            None
        );

        // Location key normalization
        assert_eq!(normalize_location_key(None, None, None), "auto");
        assert_eq!(
            normalize_location_key(Some(" Москва "), None, None),
            "city:москва"
        );
        assert_eq!(
            normalize_location_key(Some("Москва"), Some(55.7522), Some(37.6156)),
            "coords:55.752,37.616:москва"
        );
    }

    #[test]
    #[ignore]
    fn probe_live_weather_diag() {
        let cities = search_weather_cities("Москва").expect("city search failed");
        println!("=== search_weather_cities('Москва') ===\n{cities:#?}");
        assert!(!cities.is_empty());

        let auto_snap = get_weather(None, None, None, true);
        println!("=== get_weather(auto) ===\n{auto_snap:#?}");
        assert!(auto_snap.available);
        assert!(!auto_snap.hourly.is_empty());
        assert!(!auto_snap.daily.is_empty());
    }
}
