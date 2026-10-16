//! WiFi AP + captive portal + HTTP dashboard for an ABB B23 meter.

use std::collections::VecDeque;
use std::net::UdpSocket;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::prelude::Peripherals;
use esp_idf_svc::http::server::{Configuration as HttpConfig, EspHttpServer};
use esp_idf_svc::http::Method;
use esp_idf_svc::io::Write;
use esp_idf_svc::ipv4::{self, RouterConfiguration};
use esp_idf_svc::netif::{EspNetif, NetifConfiguration, NetifStack};
use esp_idf_svc::nvs::{EspDefaultNvsPartition, EspNvs, NvsDefault};
use esp_idf_svc::wifi::{
    AccessPointConfiguration, AuthMethod, BlockingWifi, Configuration, EspWifi, WifiDriver,
};
use log::{info, warn};
use pr_core::record::{Record, Summary};
use pr_core::{b23, dns};
use serde::Serialize;

mod meter;

const SSID: &str = "PowerRanger";
const AP_PASS: &str = env!("PR_AP_PASS");
const _: () = assert!(
    AP_PASS.len() >= 8 && AP_PASS.len() <= 63,
    "PR_AP_PASS: WPA2 needs 8..=63 chars"
);

// shortcut: 250 ms guessed, B23 refresh rate is undocumented; measure once wired and match it.
const PERIOD: Duration = Duration::from_millis(250);
const SLOW_EVERY: u64 = 40; // polls; kWh counters + NVS save every 10 s
const MAX_MISSES: u32 = 4;
const LIVE_LEN: usize = 1200; // 5 min at 4 Hz
const NVS_BUF: usize = 1024; // > Record::to_bytes() max (27 + 4 * 239)

/// Total, L1, L2, L3 in W; `None` = offline or phase not connected.
type Sample = [Option<f32>; 4];

#[derive(Default)]
struct State {
    seq: u64,
    inst: Option<b23::Inst>,
    energy: Option<b23::Energy>,
    link_ok: bool,
    errors: u32,
    uptime_s: u64,
    rec: Record,
    last: Option<Record>,
    live: VecDeque<Sample>,
}

#[derive(Serialize)]
struct Now {
    seq: u64,
    inst: Option<b23::Inst>,
    energy: Option<b23::Energy>,
    link_ok: bool,
    errors: u32,
    uptime_s: u64,
    rec: Summary,
    last: Option<Summary>,
}

type Json = fn(&State) -> serde_json::Result<Vec<u8>>;

fn main() -> anyhow::Result<()> {
    esp_idf_svc::sys::link_patches();
    esp_idf_svc::log::EspLogger::initialize_default();

    let p = Peripherals::take()?;
    let sysloop = EspSystemEventLoop::take()?;
    let part = EspDefaultNvsPartition::take()?;
    let mut nvs = EspNvs::new(part.clone(), "pr", true)?;

    // The run that ended with the last power loss becomes LAST RECORD if it counts.
    let last = match load(&nvs, "cur") {
        Some(cur) if cur.counts() => {
            nvs.set_raw("last", &cur.to_bytes())?;
            Some(cur)
        }
        _ => load(&nvs, "last"),
    };
    info!("last record: {:?}", last.as_ref().map(Record::summary));
    let state = Arc::new(Mutex::new(State {
        last,
        ..Default::default()
    }));

    // DHCP must hand out the AP itself as DNS (esp-idf-svc defaults to 8.8.8.8), else phones
    // never query our captive DNS and no portal pops up.
    let router = RouterConfiguration::default();
    let ap_netif = EspNetif::new_with_conf(&NetifConfiguration {
        ip_configuration: Some(ipv4::Configuration::Router(RouterConfiguration {
            dns: Some(router.subnet.gateway),
            secondary_dns: None,
            ..router
        })),
        ..NetifConfiguration::wifi_default_router()
    })?;
    let driver = WifiDriver::new(p.modem, sysloop.clone(), Some(part))?;
    let mut wifi = BlockingWifi::wrap(
        EspWifi::wrap_all(driver, EspNetif::new(NetifStack::Sta)?, ap_netif)?,
        sysloop,
    )?;
    wifi.set_configuration(&Configuration::AccessPoint(AccessPointConfiguration {
        ssid: SSID.try_into().unwrap(),
        password: AP_PASS.try_into().unwrap(),
        auth_method: AuthMethod::WPA2Personal,
        max_connections: 4,
        ..Default::default()
    }))?;
    wifi.start()?;
    wifi.wait_netif_up()?;
    let ip = wifi.wifi().ap_netif().get_ip_info()?.ip;
    info!("AP {SSID} up at {ip}");

    thread::Builder::new()
        .stack_size(4096)
        .spawn(move || dns_server(ip.octets()))?;

    // Portal sheet, OS probes and a browser tab each keep connections open; 4 sockets run out.
    let mut http = EspHttpServer::new(&HttpConfig {
        uri_match_wildcard: true,
        max_open_sockets: 7,
        ..Default::default()
    })?;
    serve(
        &mut http,
        "/",
        "text/html; charset=utf-8",
        "no-cache",
        include_bytes!("../web/index.html"),
    )?;
    serve(
        &mut http,
        "/f/mono.woff2",
        "font/woff2",
        "max-age=31536000, immutable",
        include_bytes!("../web/mono.woff2"),
    )?;
    serve(
        &mut http,
        "/f/title.woff2",
        "font/woff2",
        "max-age=31536000, immutable",
        include_bytes!("../web/title.woff2"),
    )?;
    api(&mut http, "/api", &state, |s| {
        serde_json::to_vec(&Now {
            seq: s.seq,
            inst: s.inst,
            energy: s.energy,
            link_ok: s.link_ok,
            errors: s.errors,
            uptime_s: s.uptime_s,
            rec: s.rec.summary(),
            last: s.last.as_ref().map(Record::summary),
        })
    })?;
    api(&mut http, "/api/live", &state, |s| {
        serde_json::to_vec(&s.live)
    })?;
    api(&mut http, "/api/rec", &state, |s| {
        serde_json::to_vec(&s.rec)
    })?;
    api(&mut http, "/api/last", &state, |s| {
        serde_json::to_vec(&s.last)
    })?;
    // Captive portal: every other URL (OS connectivity probes included) goes to the dashboard.
    let home: &'static str = Box::leak(format!("http://{ip}/").into_boxed_str());
    http.fn_handler("/*", Method::Get, move |req| -> anyhow::Result<()> {
        req.into_response(302, Some("Found"), &[("Location", home)])?;
        Ok(())
    })?;

    let mut m = meter::Meter::new(p.uart1, p.pins.gpio17, p.pins.gpio18, p.pins.gpio21)?;
    let t0 = Instant::now();
    let mut prev = Instant::now();
    let mut misses = 0;
    for n in 0u64.. {
        let started = Instant::now();
        let slow = n % SLOW_EVERY == 0;
        // Bus I/O outside the lock so the API never waits on a timeout.
        let inst = m.read(b23::INST_START, b23::INST_LEN);
        let energy = slow.then(|| m.read(b23::ENERGY_START, b23::ENERGY_LEN));
        let dt = prev.elapsed().as_secs_f32();
        prev = Instant::now();

        let mut s = state.lock().unwrap();
        let fresh = match inst {
            Ok(r) => {
                s.inst = b23::decode_inst(&r);
                misses = 0;
                true
            }
            Err(e) => {
                warn!("inst: {e:#}");
                s.errors += 1;
                misses += 1;
                false
            }
        };
        match energy {
            Some(Ok(r)) => s.energy = b23::decode_energy(&r),
            Some(Err(e)) => {
                warn!("energy: {e:#}");
                s.errors += 1;
            }
            None => {}
        }
        let was_ok = s.link_ok;
        s.link_ok = misses < MAX_MISSES && s.inst.is_some();
        match (was_ok, s.link_ok) {
            (false, true) => info!(
                "meter link up, L1 {:?}, f {:?}",
                s.inst.map(|i| i.l[0]),
                s.inst.and_then(|i| i.f)
            ),
            (true, false) => warn!("meter link down"),
            _ => {}
        }
        if !s.link_ok {
            // Never show stale values as live.
            s.inst = None;
            s.energy = None;
        }

        // Only fresh readings are integrated and graphed; a missed poll is a gap.
        let sample = match (fresh, s.inst) {
            (true, Some(i)) => {
                let ph = |k: usize| i.l[k].active.then_some(i.l[k].p).flatten();
                [i.p, ph(0), ph(1), ph(2)]
            }
            _ => [None; 4],
        };
        s.rec.tick(sample[0], dt);
        if s.live.len() == LIVE_LEN {
            s.live.pop_front();
        }
        s.live.push_back(sample);
        s.seq = n;
        s.uptime_s = t0.elapsed().as_secs();
        let save = (slow && n > 0).then(|| s.rec.to_bytes());
        drop(s);

        if let Some(b) = save {
            if let Err(e) = nvs.set_raw("cur", &b) {
                warn!("nvs save: {e}");
            }
        }
        thread::sleep(PERIOD.saturating_sub(started.elapsed()));
    }
    unreachable!()
}

fn load(nvs: &EspNvs<NvsDefault>, key: &str) -> Option<Record> {
    let mut buf = [0u8; NVS_BUF];
    nvs.get_raw(key, &mut buf)
        .ok()
        .flatten()
        .and_then(Record::from_bytes)
}

fn serve(
    http: &mut EspHttpServer<'static>,
    uri: &str,
    ctype: &'static str,
    cache: &'static str,
    body: &'static [u8],
) -> anyhow::Result<()> {
    http.fn_handler(uri, Method::Get, move |req| -> anyhow::Result<()> {
        req.into_response(
            200,
            None,
            &[("Content-Type", ctype), ("Cache-Control", cache)],
        )?
        .write_all(body)?;
        Ok(())
    })?;
    Ok(())
}

fn api(
    http: &mut EspHttpServer<'static>,
    uri: &str,
    state: &Arc<Mutex<State>>,
    f: Json,
) -> anyhow::Result<()> {
    let state = state.clone();
    http.fn_handler(uri, Method::Get, move |req| -> anyhow::Result<()> {
        let body = f(&state.lock().unwrap())?;
        req.into_response(
            200,
            None,
            &[
                ("Content-Type", "application/json"),
                ("Cache-Control", "no-store"),
            ],
        )?
        .write_all(&body)?;
        Ok(())
    })?;
    Ok(())
}

fn dns_server(ip: [u8; 4]) {
    let sock = match UdpSocket::bind("0.0.0.0:53") {
        Ok(s) => s,
        Err(e) => return warn!("dns bind: {e}"),
    };
    let mut buf = [0u8; 512];
    loop {
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Some(r) = dns::reply(&buf[..n], ip) {
                    let _ = sock.send_to(&r, from);
                }
            }
            Err(e) => {
                warn!("dns: {e}");
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
