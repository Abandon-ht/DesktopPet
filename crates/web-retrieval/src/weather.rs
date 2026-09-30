use super::{Source, epoch_seconds, safe_get};
use anyhow::{Context, Result, ensure};
use reqwest::Url;
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WeatherDay {
    Today,
    Tomorrow,
    DayAfter,
}

impl WeatherDay {
    fn index(self) -> usize {
        match self {
            Self::Today => 0,
            Self::Tomorrow => 1,
            Self::DayAfter => 2,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Today => "今天",
            Self::Tomorrow => "明天",
            Self::DayAfter => "后天",
        }
    }
}

pub struct WeatherReport {
    pub answer_text: String,
    pub source: Source,
}

pub async fn lookup_weather(city: &str, day: WeatherDay) -> Result<WeatherReport> {
    let city = city.trim();
    ensure!(
        (2..=80).contains(&city.chars().count()),
        "天气城市须为 2–80 字"
    );
    let (lookup_name, display_name) = geocoding_name(city);
    let mut geocoding = Url::parse("https://geocoding-api.open-meteo.com/v1/search")?;
    geocoding
        .query_pairs_mut()
        .append_pair("name", lookup_name)
        .append_pair("count", "1")
        .append_pair("language", "zh")
        .append_pair("format", "json");
    let (body, _) = safe_get(geocoding, None).await?;
    let places: GeoResponse = serde_json::from_slice(&body).context("天气城市查询响应无效")?;
    let place = places
        .results
        .unwrap_or_default()
        .into_iter()
        .next()
        .context("没有找到这个天气城市，请输入更完整的城市名称")?;

    let mut forecast = Url::parse("https://api.open-meteo.com/v1/forecast")?;
    forecast
        .query_pairs_mut()
        .append_pair("latitude", &place.latitude.to_string())
        .append_pair("longitude", &place.longitude.to_string())
        .append_pair(
            "current",
            "temperature_2m,precipitation,weather_code,wind_speed_10m",
        )
        .append_pair(
            "daily",
            "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
        )
        .append_pair("timezone", "auto")
        .append_pair("forecast_days", "3");
    let (body, _) = safe_get(forecast.clone(), None).await?;
    let data: ForecastResponse = serde_json::from_slice(&body).context("天气预报响应无效")?;
    let index = day.index();
    let date = data.daily.time.get(index).context("天气预报缺少日期")?;
    let high = data
        .daily
        .temperature_2m_max
        .get(index)
        .context("天气预报缺少最高温")?;
    let low = data
        .daily
        .temperature_2m_min
        .get(index)
        .context("天气预报缺少最低温")?;
    let code = data
        .daily
        .weather_code
        .get(index)
        .copied()
        .context("天气预报缺少天气现象")?;
    let rain = data
        .daily
        .precipitation_probability_max
        .get(index)
        .and_then(|value| *value);
    let location_name = display_name.unwrap_or(&place.name);
    let location = if let Some(country) = place.country.as_deref() {
        format!("{}（{}）", location_name, country)
    } else {
        location_name.to_string()
    };
    let mut answer = format!(
        "{}{}（{}）预计{}，{}–{}°C",
        location,
        day.label(),
        date,
        weather_description(code),
        low.round() as i32,
        high.round() as i32
    );
    if let Some(chance) = rain {
        answer.push_str(&format!("，最高降水概率 {}%", chance));
    }
    if day == WeatherDay::Today {
        if let Some(current) = data.current {
            answer.push_str(&format!(
                "。当前约 {}°C，{}，风速约 {} km/h（当地 {}）",
                current.temperature_2m.round() as i32,
                weather_description(current.weather_code),
                current.wind_speed_10m.round() as i32,
                current.time
            ));
        }
    }
    answer.push_str("。数据：Open-Meteo。[S1]");
    Ok(WeatherReport {
        answer_text: answer,
        source: Source {
            id: "S1".into(),
            title: format!("Open-Meteo 天气预报 · {location}"),
            url: forecast.to_string(),
            snippet: format!("{}，{}", date, weather_description(code)),
            retrieved_at: epoch_seconds(),
        },
    })
}

fn geocoding_name(city: &str) -> (&str, Option<&str>) {
    match city {
        // The Chinese geocoder returns no result for 纽约. Searching "New York"
        // with language=zh can even rank York, Nebraska first.
        "纽约" | "紐約" | "纽约市" | "紐約市" => ("New York City", Some("纽约")),
        _ => (city, None),
    }
}

fn weather_description(code: u8) -> &'static str {
    match code {
        0 => "晴",
        1 => "大致晴朗",
        2 => "局部多云",
        3 => "阴",
        45 | 48 => "雾",
        51..=57 => "毛毛雨",
        61..=67 => "雨",
        71..=77 => "雪",
        80..=82 => "阵雨",
        85 | 86 => "阵雪",
        95..=99 => "雷暴",
        _ => "天气变化",
    }
}

#[derive(Deserialize)]
struct GeoResponse {
    results: Option<Vec<GeoPlace>>,
}
#[derive(Deserialize)]
struct GeoPlace {
    name: String,
    country: Option<String>,
    latitude: f64,
    longitude: f64,
}
#[derive(Deserialize)]
struct ForecastResponse {
    current: Option<Current>,
    daily: Daily,
}
#[derive(Deserialize)]
struct Current {
    time: String,
    temperature_2m: f32,
    weather_code: u8,
    wind_speed_10m: f32,
}
#[derive(Deserialize)]
struct Daily {
    time: Vec<String>,
    weather_code: Vec<u8>,
    temperature_2m_max: Vec<f32>,
    temperature_2m_min: Vec<f32>,
    precipitation_probability_max: Vec<Option<u8>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_weather_codes() {
        assert_eq!(weather_description(0), "晴");
        assert_eq!(weather_description(63), "雨");
        assert_eq!(weather_description(95), "雷暴");
        assert_eq!(geocoding_name("纽约"), ("New York City", Some("纽约")));
    }

    #[tokio::test]
    #[ignore = "requires Open-Meteo access"]
    async fn live_shanghai_weather() {
        let report = lookup_weather("上海", WeatherDay::Today).await.unwrap();
        assert!(report.answer_text.contains("上海"));
        assert!(report.answer_text.contains("°C"));
        assert!(report.source.url.starts_with("https://api.open-meteo.com/"));
    }
}
