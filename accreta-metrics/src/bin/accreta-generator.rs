//! Continuous synthetic metrics generator for the accreta-metrics demo service.
//!
//! The generator deliberately talks to the public HTTP API. This makes it useful as an
//! end-to-end smoke-test source for the same ingestion path used by real clients.

use std::env;
use std::io;
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Timelike, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{MissedTickBehavior, interval};

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8080";
const DEFAULT_USER: &str = "demo";
const DEFAULT_PASSWORD: &str = "demo123";

const SERVICES: &[&str] = &["api", "payments", "auth", "search"];
const REGIONS: &[&str] = &[
    "eu-west",
    "eu-central",
    "us-east",
    "us-west",
    "ap-southeast",
];
const ENDPOINTS: &[&str] = &["/users", "/orders", "/search", "/checkout"];
//const STATUSES: &[&str] = &["200", "400", "404", "500"];

#[derive(Debug, Clone, Copy)]
enum Mode {
    Realtime,
    Accelerated,
}

#[derive(Debug, Clone, Copy)]
enum Scenario {
    Normal,
    TrafficSpike,
    LatencySpike,
    ErrorSpike,
    Mixed,
}

impl Scenario {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "normal" => Ok(Self::Normal),
            "traffic-spike" => Ok(Self::TrafficSpike),
            "latency-spike" => Ok(Self::LatencySpike),
            "error-spike" => Ok(Self::ErrorSpike),
            "mixed" => Ok(Self::Mixed),
            _ => Err(format!("unknown scenario '{value}'")),
        }
    }
}

#[derive(Debug)]
struct Config {
    base_url: String,
    username: String,
    password: String,
    events_per_second: u32,
    interval: Duration,
    mode: Mode,
    time_scale: f64,
    seed: u64,
    scenario: Scenario,
    batch_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: env::var("ACCRETA_METRICS_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.into()),
            username: env::var("ACCRETA_METRICS_USERNAME").unwrap_or_else(|_| DEFAULT_USER.into()),
            password: env::var("ACCRETA_METRICS_PASSWORD")
                .unwrap_or_else(|_| DEFAULT_PASSWORD.into()),
            events_per_second: 100,
            interval: Duration::from_secs(1),
            mode: Mode::Realtime,
            time_scale: 60.0,
            seed: 42,
            scenario: Scenario::Normal,
            batch_size: 100,
        }
    }
}

fn next_value<I>(args: &mut I, name: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| format!("missing value for {name}"))
}

impl Config {
    fn parse() -> Result<Self, String> {
        let mut cfg = Self::default();
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--url" => cfg.base_url = next_value(&mut args, "--url")?,
                "--username" => cfg.username = next_value(&mut args, "--username")?,
                "--password" => cfg.password = next_value(&mut args, "--password")?,
                "--events-per-second" => {
                    cfg.events_per_second =
                        next_value(&mut args, "--events-per-second")?
                            .parse()
                            .map_err(|_| "invalid --events-per-second".to_string())?;
                }
                "--interval-ms" => {
                    let ms: u64 = next_value(&mut args, "--interval-ms")?
                        .parse()
                        .map_err(|_| "invalid --interval-ms".to_string())?;
                    cfg.interval = Duration::from_millis(ms.max(1));
                }
                "--mode" => {
                    cfg.mode = match next_value(&mut args, "--mode")?.as_str() {
                        "realtime" => Mode::Realtime,
                        "accelerated" => Mode::Accelerated,
                        other => return Err(format!("unknown mode '{other}'")),
                    };
                }
                "--time-scale" => {
                    cfg.time_scale = next_value(&mut args, "--time-scale")?
                        .parse()
                        .map_err(|_| "invalid --time-scale".to_string())?
                }
                "--seed" => {
                    cfg.seed = next_value(&mut args, "--seed")?
                        .parse()
                        .map_err(|_| "invalid --seed".to_string())?
                }
                "--scenario" => {
                    cfg.scenario = Scenario::parse(&next_value(&mut args, "--scenario")?)?
                }
                "--batch-size" => {
                    cfg.batch_size = next_value(&mut args, "--batch-size")?
                        .parse()
                        .map_err(|_| "invalid --batch-size".to_string())?
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => return Err(format!("unknown argument '{other}'")),
            }
        }
        if cfg.events_per_second == 0 || cfg.batch_size == 0 || cfg.time_scale <= 0.0 {
            return Err("events-per-second, batch-size and time-scale must be positive".into());
        }
        Ok(cfg)
    }
}

fn print_help() {
    println!(
        "accreta-generator\n\n\
         Usage: cargo run -p accreta-metrics --bin accreta-generator -- [options]\n\n\
         Options:\n\
           --url URL                 API base URL (default http://127.0.0.1:8080)\n\
           --username NAME           Login username (default demo)\n\
           --password PASSWORD       Login password (default demo123)\n\
           --events-per-second N     Synthetic observations per second (default 100)\n\
           --interval-ms N           Generation tick interval (default 1000)\n\
           --mode realtime|accelerated\n\
           --time-scale N            Simulated seconds per real second in accelerated mode (default 60)\n\
           --seed N                  Deterministic seed (default 42)\n\
           --scenario normal|traffic-spike|latency-spike|error-spike|mixed\n\
           --batch-size N            Samples sent per HTTP request (default 100)\n"
    );
}

#[derive(Debug, Deserialize)]
struct LoginResponse {
    token: String,
}

#[derive(Debug, Deserialize)]
struct HttpResponse {
    status: u16,
    body: String,
}

/// Tiny deterministic PRNG. Keeping the generator self-contained avoids adding a runtime
/// dependency just for synthetic data. This is not intended for cryptographic use.
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn unit(&mut self) -> f64 {
        (self.next_u64() as f64) / (u64::MAX as f64)
    }

    fn usize(&mut self, upper: usize) -> usize {
        (self.next_u64() as usize) % upper
    }

    fn normalish(&mut self) -> f64 {
        // Irwin-Hall approximation: bounded, cheap and sufficient for demo traffic.
        let mut sum = 0.0;
        for _ in 0..6 {
            sum += self.unit();
        }
        sum - 3.0
    }
}

struct Generator {
    rng: Rng,
    simulated_now: DateTime<Utc>,
    scenario: Scenario,
}

impl Generator {
    fn new(seed: u64, scenario: Scenario) -> Self {
        Self {
            rng: Rng::new(seed),
            simulated_now: Utc::now(),
            scenario,
        }
    }

    fn advance(&mut self, real_elapsed: Duration, mode: Mode, scale: f64) {
        let seconds = real_elapsed.as_secs_f64()
            * match mode {
                Mode::Realtime => 1.0,
                Mode::Accelerated => scale,
            };
        let millis = (seconds * 1000.0) as i64;
        self.simulated_now += ChronoDuration::milliseconds(millis.max(1));
    }

    fn sample(&mut self) -> Value {
        let service = SERVICES[self.rng.usize(SERVICES.len())];
        let region = REGIONS[self.rng.usize(REGIONS.len())];
        let endpoint = ENDPOINTS[self.rng.usize(ENDPOINTS.len())];

        let hour = self.simulated_now.hour() as f64 + self.simulated_now.minute() as f64 / 60.0;
        let daily = ((hour - 14.0) / 5.0).cos();
        let mut traffic = 1.0 + 0.45 * daily;
        let incident = self.incident_factor();
        traffic *= incident.traffic;

        let service_factor = match service {
            "payments" => 1.20,
            "search" => 0.90,
            "auth" => 0.75,
            _ => 1.0,
        };
        let requests =
            (traffic * service_factor * (1.0 + 0.20 * self.rng.normalish())).max(0.05) * 100.0;

        let base_latency = match endpoint {
            "/search" => 70.0,
            "/checkout" => 95.0,
            "/orders" => 45.0,
            _ => 25.0,
        };
        let region_latency = match region {
            "ap-southeast" => 18.0,
            "us-west" => 8.0,
            "eu-central" => 5.0,
            _ => 0.0,
        };
        let latency =
            (base_latency + region_latency + 10.0 * self.rng.normalish()) * incident.latency;

        let error_probability = (match endpoint {
            "/checkout" => 0.018,
            "/orders" => 0.012,
            _ => 0.008,
        } * incident.errors)
            .min(0.35);
        let error = self.rng.unit() < error_probability;
        let status = if error {
            if self.rng.unit() < 0.70 { "500" } else { "400" }
        } else if self.rng.unit() < 0.015 {
            "404"
        } else {
            "200"
        };

        json!({
            "ts": self.simulated_now.to_rfc3339(),
            "measures": [requests.round() as u64, latency.max(1.0), if error { 1u64 } else { 0u64 }],
            "dimensions": [service, region, endpoint, status]
        })
    }

    fn incident_factor(&self) -> IncidentFactor {
        let active = (self.simulated_now.timestamp() / 300) % 12 == 4;
        if !active {
            return IncidentFactor::default();
        }
        match self.scenario {
            Scenario::Normal => IncidentFactor::default(),
            Scenario::TrafficSpike => IncidentFactor {
                traffic: 4.0,
                ..Default::default()
            },
            Scenario::LatencySpike => IncidentFactor {
                latency: 3.0,
                ..Default::default()
            },
            Scenario::ErrorSpike => IncidentFactor {
                errors: 10.0,
                ..Default::default()
            },
            Scenario::Mixed => IncidentFactor {
                traffic: 2.5,
                latency: 2.2,
                errors: 7.0,
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct IncidentFactor {
    traffic: f64,
    latency: f64,
    errors: f64,
}
impl Default for IncidentFactor {
    fn default() -> Self {
        Self {
            traffic: 1.0,
            latency: 1.0,
            errors: 1.0,
        }
    }
}

async fn request(
    method: &str,
    url: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> io::Result<HttpResponse> {
    let without_scheme = url.strip_prefix("http://").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "only http:// URLs are supported",
        )
    })?;
    let (host_port, path) = if let Some((host, p)) = without_scheme.split_once('/') {
        (host, format!("/{p}"))
    } else {
        (without_scheme, "/".to_owned())
    };
    let mut stream = TcpStream::connect(host_port).await?;
    let body = body.unwrap_or("");
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {host_port}\r\nContent-Type: application/json\r\nConnection: close\r\n{auth}Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await?;
    let text = String::from_utf8_lossy(&bytes);
    let mut parts = text.splitn(2, "\r\n\r\n");
    let headers = parts.next().unwrap_or("");
    let response_body = parts.next().unwrap_or("").to_owned();
    let status = headers
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    Ok(HttpResponse {
        status,
        body: response_body,
    })
}

async fn login(cfg: &Config) -> Result<String, Box<dyn std::error::Error>> {
    let body = serde_json::to_string(&json!({"username": cfg.username, "password": cfg.password}))?;
    let response = request(
        "POST",
        &format!("{}/login", cfg.base_url.trim_end_matches('/')),
        None,
        Some(&body),
    )
    .await?;
    if response.status != 200 {
        return Err(format!("login failed (HTTP {}): {}", response.status, response.body).into());
    }
    Ok(serde_json::from_str::<LoginResponse>(&response.body)?.token)
}

async fn create_schema(cfg: &Config, token: &str) -> Result<(), Box<dyn std::error::Error>> {
    let body = json!({
        "name": "demo-observability",
        "dimensions": ["service", "region", "endpoint", "status"],
        "measures": [
            {"name": "request_count", "value_type": "u64", "aggregates": ["sum", "count"]},
            {"name": "latency_ms", "value_type": "f64", "aggregates": ["sum", "count", "min", "max", "tdigest"]},
            {"name": "error_count", "value_type": "u64", "aggregates": ["sum", "count"]}
        ],
        "retention": {"second": 1200, "minute": 72000, "hour": 172800, "day": 5256000}
    });
    let body = serde_json::to_string(&body)?;
    let response = request(
        "POST",
        &format!("{}/schema", cfg.base_url.trim_end_matches('/')),
        Some(token),
        Some(&body),
    )
    .await?;
    match response.status {
        201 | 409 => Ok(()),
        status => Err(format!("schema creation failed (HTTP {status}): {}", response.body).into()),
    }
}

async fn ingest(
    cfg: &Config,
    token: &str,
    samples: &[Value],
) -> Result<usize, Box<dyn std::error::Error>> {
    let body = serde_json::to_string(&json!({"samples": samples}))?;
    let response = request(
        "POST",
        &format!("{}/schema/ingest", cfg.base_url.trim_end_matches('/')),
        Some(token),
        Some(&body),
    )
    .await?;
    if response.status != 200 {
        return Err(format!(
            "ingest failed (HTTP {}): {}",
            response.status, response.body
        )
        .into());
    }
    #[derive(Deserialize)]
    struct IngestResponse {
        ingested: usize,
    }
    Ok(serde_json::from_str::<IngestResponse>(&response.body)?.ingested)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::parse().map_err(|e| format!("{e}\nUse --help for options."))?;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!(url = %cfg.base_url, ?cfg.mode, ?cfg.scenario, seed = cfg.seed, events_per_second = cfg.events_per_second, "starting synthetic metrics generator");

    let token = login(&cfg).await?;
    create_schema(&cfg, &token).await?;
    tracing::info!("demo schema ready");

    let mut generator = Generator::new(cfg.seed, cfg.scenario);
    let mut ticker = interval(cfg.interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut total = 0usize;
    let mut last = std::time::Instant::now();

    loop {
        ticker.tick().await;
        let elapsed = last.elapsed();
        last = std::time::Instant::now();
        generator.advance(elapsed, cfg.mode, cfg.time_scale);

        let events = ((cfg.events_per_second as f64) * cfg.interval.as_secs_f64())
            .round()
            .max(1.0) as usize;
        let mut samples = Vec::with_capacity(events.min(cfg.batch_size));
        let mut sent = 0usize;
        for _ in 0..events {
            samples.push(generator.sample());
            if samples.len() >= cfg.batch_size {
                sent += ingest(&cfg, &token, &samples).await?;
                samples.clear();
            }
        }
        if !samples.is_empty() {
            sent += ingest(&cfg, &token, &samples).await?;
        }
        total += sent;

        tracing::info!(simulated_at = %generator.simulated_now.to_rfc3339(), batch = sent, total, "generated synthetic observations");
    }
}
