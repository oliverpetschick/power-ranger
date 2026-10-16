//! WiFi AP + HTTP dashboard for an ABB B23 meter.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::prelude::Peripherals;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{
    AccessPointConfiguration, AuthMethod, BlockingWifi, Configuration, EspWifi,
};
use log::{info, warn};
use serde::Serialize;

mod meter;

const SSID: &str = "PowerRanger";
const AP_PASS: &str = env!("PR_AP_PASS");
const _: () = assert!(AP_PASS.len() >= 8 && AP_PASS.len() <= 63, "PR_AP_PASS: WPA2 needs 8..=63 chars");
const INDEX: &str = include_str!("../web/index.html");
const ENERGY_EVERY: u64 = 10; // poll cycles
const MAX_MISSES: u32 = 3;

#[derive(Default, Serialize)]
struct State {
    inst: Option<b23::Inst>,
    energy: Option<b23::Energy>,
    link_ok: bool,
    errors: u32,
    uptime_s: u64,
}

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let p = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(EspWifi::new(p.modem, sysloop.clone(), Some(nvs))?, sysloop)?;
    wifi.set_configuration(&Configuration::AccessPoint(AccessPointConfiguration {
        ssid: SSID.try_into().unwrap(),
        password: AP_PASS.try_into().unwrap(),
        auth_method: AuthMethod::WPA2Personal,
        max_connections: 4,
        ..Default::default()
    }))?;
    wifi.start()?;
    wifi.wait_netif_up()?;
    info!("AP {SSID} up at {}", wifi.wifi().ap_netif().get_ip_info()?.ip);

    let state = Arc::new(Mutex::new(State::default()));
    let mut http = EspHttpServer::new(&HttpConfig::default())?;
    http.fn_handler("/", Method::Get, |req| -> anyhow::Result<()> {
        req.into_ok_response()?.write_all(INDEX.as_bytes())?;
        Ok(())
    })?;
    let s = state.clone();
    http.fn_handler("/api", Method::Get, move |req| -> anyhow::Result<()> {
        let body = serde_json::to_vec(&*s.lock().unwrap())?;
        req.into_response(200, None, &[("Content-Type", "application/json")])?
            .write_all(&body)?;
        Ok(())
    })?;

    let mut m = meter::Meter::new(p.uart1, p.pins.gpio17, p.pins.gpio18, p.pins.gpio21)?;
    let t0 = Instant::now();
    let mut misses = 0;
    for n in 0u64.. {
        // Bus I/O outside the lock so /api never waits on a timeout.
        let inst = m.read(b23::INST_START, b23::INST_LEN);
        let energy = (n % ENERGY_EVERY == 0).then(|| m.read(b23::ENERGY_START, b23::ENERGY_LEN));

        let mut s = state.lock().unwrap();
        match inst {
            Ok(r) => {
                s.inst = b23::decode_inst(&r);
                misses = 0;
            }
            Err(e) => {
                warn!("inst: {e:#}");
                s.errors += 1;
                misses += 1;
            }
        }
        match energy {
            Some(Ok(r)) => s.energy = b23::decode_energy(&r),
            Some(Err(e)) => {
                warn!("energy: {e:#}");
                s.errors += 1;
            }
            None => {}
        }
        s.link_ok = misses < MAX_MISSES;
        if !s.link_ok {
            // Never show stale values as live.
            s.inst = None;
            s.energy = None;
        }
        s.uptime_s = t0.elapsed().as_secs();
        drop(s);

        thread::sleep(Duration::from_secs(1));
    }
    unreachable!()
}
