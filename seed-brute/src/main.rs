use bip39::{Mnemonic, Language};
use bitcoin::util::bip32::ExtendedPrivKey;
use bitcoin::network::constants::Network;
use getrandom::getrandom;
use clap::{Arg, Command};
use env_logger::Env;
use log::{info, error};
use rayon::prelude::*;
use std::process::{Command as SysCommand, Stdio};
use std::io::Read;
use std::time::Duration;
use wait_timeout::ChildExt;
use std::thread;

use prometheus::{Encoder, IntCounter, Histogram, TextEncoder};
use lazy_static::lazy_static;

use hyper::{Body, Response, Server, Request, Method, StatusCode};
use hyper::service::{make_service_fn, service_fn};
use tokio::runtime::Builder;

lazy_static! {
    static ref SEED_COUNTER: IntCounter = prometheus::register_int_counter!(
        "seeds_checked", "Number of seeds checked"
    ).unwrap();
    static ref SUCCESS_COUNTER: IntCounter = prometheus::register_int_counter!(
        "seeds_success", "Number of seeds with found balance"
    ).unwrap();
    static ref CHECK_HISTOGRAM: Histogram = prometheus::register_histogram!(
        "seed_check_duration_seconds", "Duration of seed check (s)"
    ).unwrap();
}

/// Запускает HTTP-сервер на /metrics для Prometheus
fn spawn_metrics_server() {
    thread::spawn(|| {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let make_svc = make_service_fn(|_| async {
                Ok::<_, hyper::Error>(service_fn(|req: Request<Body>| async move {
                    if req.method() == Method::GET && req.uri().path() == "/metrics" {
                        let mut buffer = Vec::new();
                        let encoder = TextEncoder::new();
                        let mf = prometheus::gather();
                        encoder.encode(&mf, &mut buffer).unwrap();
                        Ok::<_, hyper::Error>(Response::new(Body::from(buffer)))
                    } else {
                        let mut not_found = Response::default();
                        *not_found.status_mut() = StatusCode::NOT_FOUND;
                        Ok::<_, hyper::Error>(not_found)
                    }
                }))
            });
            let addr = ([0, 0, 0, 0], 9184).into();
            let server = Server::bind(&addr).serve(make_svc);
            info!("Metrics server listening on http://{}", addr);
            if let Err(e) = server.await {
                error!("Metrics server error: {}", e);
            }
        });
    });
}

/// Вызывает внешний Python-скрипт для проверки баланса
fn call_rpc_checker(seed: &str, target: &str, timeout: Duration) -> Option<String> {
    let mut child = match SysCommand::new("python3")
        .arg("../rpc_checker.py")
        .arg(seed)
        .arg(target)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to spawn rpc_checker: {}", e);
            return None;
        }
    };
    match child.wait_timeout(timeout).expect("Failed to wait on child") {
        Some(status) => {
            let mut out = String::new();
            if let Some(mut stdout) = child.stdout.take() {
                stdout.read_to_string(&mut out).ok();
            }
            if !status.success() {
                error!("rpc_checker exited with {:?}, output: {}", status.code(), out);
                return None;
            }
            Some(out)
        }
        None => {
            error!("rpc_checker timed out");
            let _ = child.kill();
            None
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Инициализация логирования
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    // Парсинг CLI
    let matches = Command::new("seed-brute")
        .version("0.1.0")
        .about("Brute-force lost crypto seed phrases")
        .arg(
            Arg::new("count")
                .short('c')
                .long("count")
                .value_name("NUM")
                .help("Number of seeds to generate/check")
                .default_value("100")
        )
        .arg(
            Arg::new("threads")
                .short('t')
                .long("threads")
                .value_name("N")
                .help("Number of parallel threads")
                .default_value("1")
        )
        .arg(
            Arg::new("destination")
                .short('d')
                .long("destination")
                .value_name("ADDRESS")
                .help("Target address to send found funds to")
                .required(true)
        )
        .arg(
            Arg::new("timeout")
                .short('o')
                .long("timeout")
                .value_name("SECS")
                .help("Timeout in seconds for balance check per seed")
                .default_value("30")
        )
        .get_matches();

    let count: usize = matches.get_one::<String>("count").unwrap().parse()?;
    let threads: usize = matches.get_one::<String>("threads").unwrap().parse()?;
    let destination = matches.get_one::<String>("destination").unwrap();
    let timeout_secs: u64 = matches.get_one::<String>("timeout").unwrap().parse()?;
    let timeout = Duration::from_secs(timeout_secs);

    info!("Starting brute-force: count={} threads={} timeout={}s", count, threads, timeout_secs);

    // Запуск метрик
    spawn_metrics_server();

    // Конфигурируем Rayon
    rayon::ThreadPoolBuilder::new().num_threads(threads).build_global()?;

    // Основной цикл перебора
    (0..count).into_par_iter().for_each(|i| {
        let _timer = CHECK_HISTOGRAM.start_timer();

        // 1) Энтропия + мнемоника
        let mut entropy = [0u8; 16];
        if let Err(e) = getrandom(&mut entropy) {
            error!("[{}] Entropy error: {}", i, e);
            return;
        }
        let mnemonic = match Mnemonic::from_entropy_in(Language::English, &entropy) {
            Ok(m) => m,
            Err(e) => {
                error!("[{}] Mnemonic error: {}", i, e);
                return;
            }
        };
        let phrase = mnemonic.to_string();
        let seed = mnemonic.to_seed("");
        let xprv = match ExtendedPrivKey::new_master(Network::Bitcoin, &seed) {
            Ok(k) => k,
            Err(e) => {
                error!("[{}] XPRV derivation error: {}", i, e);
                return;
            }
        };
        info!("[{}] {} -> {}", i, phrase, xprv);

        // 2) Проверка баланса через Python
        let output = call_rpc_checker(&phrase, destination, timeout);

        // 3) Метрики и логика успеха
        SEED_COUNTER.inc();
        if let Some(ref out) = output {
            if out.contains("Found balance") {
                SUCCESS_COUNTER.inc();
                info!("[{}] SUCCESS: {}", i, phrase);
            }
        }
    });

    info!("Brute-force complete");
    Ok(())
}

