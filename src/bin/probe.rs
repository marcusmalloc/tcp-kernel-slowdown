use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process::ExitCode;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(10);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((target, samples)) = parse_args(&args) else {
        eprintln!("usage: probe <host:port> [samples]");
        return ExitCode::from(2);
    };
    match run(target, samples) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("probe: {err}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: &[String]) -> Option<(&str, usize)> {
    match args {
        [target] => Some((target.as_str(), 5)),
        [target, samples] => Some((target.as_str(), samples.parse().ok().filter(|&n| n > 0)?)),
        _ => None,
    }
}

fn run(target: &str, samples: usize) -> io::Result<()> {
    let addr = target
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| io::Error::other(format!("{target} resolved to no address")))?;
    let request = format!(
        "GET / HTTP/1.1\r\nHost: {target}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    let mut pings = Vec::with_capacity(samples);
    let mut upgrades = Vec::with_capacity(samples);
    for i in 1..=samples {
        let (ping, upgrade) = sample(addr, request.as_bytes())?;
        print_row(&format!("sample {i}"), ping, upgrade);
        pings.push(ping);
        upgrades.push(upgrade);
    }
    print_row("median", median(&mut pings), median(&mut upgrades));
    Ok(())
}

fn sample(addr: SocketAddr, request: &[u8]) -> io::Result<(Duration, Duration)> {
    let start = Instant::now();
    let mut stream = TcpStream::connect_timeout(&addr, TIMEOUT)?;
    let ping = start.elapsed();

    stream.set_read_timeout(Some(TIMEOUT))?;
    let mut status = String::new();
    let start = Instant::now();
    stream.write_all(request)?;
    BufReader::new(&stream).read_line(&mut status)?;
    let upgrade = start.elapsed();

    if !status.starts_with("HTTP/1.1 101") {
        return Err(io::Error::other(format!(
            "expected HTTP/1.1 101, got {:?}",
            status.trim_end()
        )));
    }
    Ok((ping, upgrade))
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort();
    let n = samples.len();
    (samples[(n - 1) / 2] + samples[n / 2]) / 2
}

fn print_row(label: &str, ping: Duration, upgrade: Duration) {
    let ms = |duration: Duration| duration.as_secs_f64() * 1000.0;
    println!(
        "{label}: tcp ping {:.2} ms, upgrade {:.2} ms",
        ms(ping),
        ms(upgrade)
    );
}
