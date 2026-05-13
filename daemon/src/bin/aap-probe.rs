// AAP command probe. Connects to AirPods on PSM 0x1001, performs handshake,
// then sends each provided hex packet 500ms apart and prints raw responses.
// Holds the connection open for 20s afterwards so we can observe BlueZ /
// PipeWire profile / transport changes from another shell.
//
// Usage:
//   aap-probe <MAC> <hex-packet> [<hex-packet> ...]
//
// Example (enable Hearing Aid mode):
//   aap-probe 2C:18:09:E9:19:65 04000400090002C0100000000

use bluer::l2cap::{SeqPacket, Socket, SocketAddr};
use bluer::Address;
use std::env;
use std::str::FromStr;
use std::time::Duration;
use tokio::time::{sleep, timeout};

const AAP_PSM: u16 = 0x1001;
const HANDSHAKE: [u8; 16] = [
    0x00, 0x00, 0x04, 0x00, 0x01, 0x00, 0x02, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
const SET_FEATURES: [u8; 14] = [
    0x04, 0x00, 0x04, 0x00, 0x4D, 0x00, 0xD7, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
const SUBSCRIBE: [u8; 10] = [
    0x04, 0x00, 0x04, 0x00, 0x0F, 0x00, 0xFF, 0xFF, 0xFF, 0xFF,
];

fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.len() % 2 != 0 {
        return Err(format!("odd hex length in {s:?}"));
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&cleaned[i..i + 2], 16)
            .map_err(|e| format!("bad hex {:?}: {e}", &cleaned[i..i + 2])))
        .collect()
}

fn fmt_bytes(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: aap-probe <MAC> <hex-packet> [<hex-packet> ...]");
        std::process::exit(2);
    }
    let address = Address::from_str(&args[1])?;
    let packets: Vec<Vec<u8>> = args[2..]
        .iter()
        .map(|s| parse_hex(s))
        .collect::<Result<_, _>>()?;

    println!("[probe] connecting to {address} PSM 0x{AAP_PSM:04X}");
    let sock = Socket::new_seq_packet()?;
    let addr = SocketAddr::new(address, bluer::AddressType::BrEdr, AAP_PSM);
    let seq: SeqPacket = sock.connect(addr).await?;
    println!("[probe] L2CAP connected");
    sleep(Duration::from_millis(500)).await;

    seq.send(&HANDSHAKE).await?;
    println!("[probe] sent HANDSHAKE");
    let mut buf = vec![0u8; 1024];
    match timeout(Duration::from_secs(2), seq.recv(&mut buf)).await {
        Ok(Ok(n)) => println!("[probe] handshake reply ({} bytes): {}", n, fmt_bytes(&buf[..n])),
        Ok(Err(e)) => println!("[probe] handshake recv error: {e}"),
        Err(_) => println!("[probe] handshake recv timeout"),
    }

    seq.send(&SET_FEATURES).await?;
    println!("[probe] sent SET_FEATURES");
    if let Ok(Ok(n)) = timeout(Duration::from_secs(1), seq.recv(&mut buf)).await {
        println!("[probe] features reply ({} bytes): {}", n, fmt_bytes(&buf[..n]));
    }

    seq.send(&SUBSCRIBE).await?;
    println!("[probe] sent SUBSCRIBE_NOTIFICATIONS");
    if let Ok(Ok(n)) = timeout(Duration::from_secs(1), seq.recv(&mut buf)).await {
        println!("[probe] subscribe reply ({} bytes): {}", n, fmt_bytes(&buf[..n]));
    }

    sleep(Duration::from_millis(500)).await;

    for (i, pkt) in packets.iter().enumerate() {
        println!("\n[probe] === packet {}/{}  send: {}", i + 1, packets.len(), fmt_bytes(pkt));
        seq.send(pkt).await?;
        // Listen 2s for any responses
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            let now = tokio::time::Instant::now();
            if now >= deadline { break; }
            match timeout(deadline - now, seq.recv(&mut buf)).await {
                Ok(Ok(0)) => { println!("[probe] connection closed"); return Ok(()); }
                Ok(Ok(n)) => println!("[probe]   recv ({} bytes): {}", n, fmt_bytes(&buf[..n])),
                Ok(Err(e)) => { println!("[probe] recv error: {e}"); return Ok(()); }
                Err(_) => break,
            }
        }
    }

    println!("\n[probe] holding connection open for 20s (observe pactl / btmon from another shell)…");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline { break; }
        match timeout(deadline - now, seq.recv(&mut buf)).await {
            Ok(Ok(0)) => { println!("[probe] connection closed by peer"); break; }
            Ok(Ok(n)) => println!("[probe]   notif ({} bytes): {}", n, fmt_bytes(&buf[..n])),
            Ok(Err(e)) => { println!("[probe] recv error: {e}"); break; }
            Err(_) => break,
        }
    }
    println!("[probe] done");
    Ok(())
}
