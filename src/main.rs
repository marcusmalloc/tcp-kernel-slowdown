use std::collections::VecDeque;
use std::convert::Infallible;
use std::io::{self, ErrorKind};
use std::process::{Command, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use nfq::{Message, Queue, Verdict};

const QUEUE_MAX_LEN: u32 = 65536;
const POLL_INTERVAL: Duration = Duration::from_millis(1);

struct Held {
    msg: Message,
    release_at: Instant,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((port, delay)) = parse_args(&args) else {
        eprintln!("usage: tcp-kernel-slowdown <port> <delay-ms>");
        return ExitCode::from(2);
    };
    let Err(err) = run(port, delay);
    eprintln!("tcp-kernel-slowdown: {err}");
    ExitCode::FAILURE
}

fn parse_args(args: &[String]) -> Option<(u16, Duration)> {
    let [port, delay_ms] = args else {
        return None;
    };
    Some((
        port.parse().ok()?,
        Duration::from_millis(delay_ms.parse().ok()?),
    ))
}

fn run(port: u16, delay: Duration) -> io::Result<Infallible> {
    ensure_rule(port)?;
    let mut queue = Queue::open()?;
    // Each port gets its own queue, so a rule left from an earlier port has no reader and --queue-bypass passes it.
    queue.bind(port)?;
    queue.set_queue_max_len(port, QUEUE_MAX_LEN)?;
    queue.set_fail_open(port, true)?;

    let mut held: VecDeque<Held> = VecDeque::new();
    loop {
        let now = Instant::now();
        while let Some(mut due) = held.pop_front_if(|h| h.release_at <= now) {
            due.msg.set_verdict(Verdict::Accept);
            queue.verdict(due.msg)?;
        }
        queue.set_nonblocking(!held.is_empty());
        match queue.recv() {
            Ok(msg) => held.push_back(Held {
                msg,
                release_at: Instant::now() + delay,
            }),
            // nfq::Queue exposes no fd for poll(2), so the loop polls recv while packets are held.
            Err(err) if err.kind() == ErrorKind::WouldBlock => {
                let wait = held[0].release_at.saturating_duration_since(Instant::now());
                thread::sleep(wait.min(POLL_INTERVAL));
            }
            Err(err) => return Err(err),
        }
    }
}

fn ensure_rule(port: u16) -> io::Result<()> {
    let rule =
        format!("INPUT -p tcp --dport {port} --syn -j NFQUEUE --queue-num {port} --queue-bypass");
    let iptables = |op: &str| {
        let mut command = Command::new("iptables");
        command.arg(op).args(rule.split_whitespace());
        command
    };
    if iptables("-C").stderr(Stdio::null()).status()?.success() {
        return Ok(());
    }
    if iptables("-I").status()?.success() {
        return Ok(());
    }
    Err(io::Error::other(format!("iptables -I {rule} failed")))
}
