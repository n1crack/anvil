//! Minimal DNS responder: answers A 127.0.0.1 for `*.<tld>`, REFUSED for anything else.

use std::net::UdpSocket;

/// `tld` is read for every query, so a changed suffix applies without a new server.
pub fn serve(port: u16, tld: &std::sync::RwLock<String>) -> std::io::Result<()> {
    let sock = UdpSocket::bind(("127.0.0.1", port))?;
    let mut buf = [0u8; 512];
    loop {
        let (n, peer) = sock.recv_from(&mut buf)?;
        let resp = answer(&buf[..n], &tld.read().unwrap_or_else(|e| e.into_inner()));
        if let Some(resp) = resp {
            let _ = sock.send_to(&resp, peer);
        }
    }
}

/// Builds a response for a single-question query. `None` = malformed, drop it.
pub fn answer(q: &[u8], tld: &str) -> Option<Vec<u8>> {
    if q.len() < 12 || u16::from_be_bytes([q[4], q[5]]) != 1 {
        return None;
    }
    // Walk the question name.
    let mut i = 12;
    let mut labels = Vec::new();
    loop {
        let len = *q.get(i)? as usize;
        i += 1;
        if len == 0 {
            break;
        }
        labels.push(String::from_utf8_lossy(q.get(i..i + len)?).to_ascii_lowercase());
        i += len;
    }
    let qtype = u16::from_be_bytes([*q.get(i)?, *q.get(i + 1)?]);
    let question = q.get(12..i + 4)?;

    let ours = labels.last().is_some_and(|l| l == tld);
    let a = ours && qtype == 1;
    let rcode = if ours { 0 } else { 5 }; // NOERROR / REFUSED

    let mut r = Vec::with_capacity(i + 20);
    r.extend_from_slice(&q[0..2]); // id
    r.push(0x84 | (q[2] & 0x01)); // QR, AA, copy RD
    r.push(rcode);
    r.extend_from_slice(&[0, 1, 0, a as u8, 0, 0, 0, 0]);
    r.extend_from_slice(question);
    if a {
        // name ptr -> offset 12, A, IN, ttl 60, rdlen 4, 127.0.0.1
        r.extend_from_slice(&[0xC0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 127, 0, 0, 1]);
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::{answer, serve};

    fn query(name: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0xAB, 0xCD, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for l in name.split('.') {
            q.push(l.len() as u8);
            q.extend_from_slice(l.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&[0, 1]);
        q
    }

    #[test]
    fn answers_tld_a_records() {
        let r = answer(&query("Shop.Anvil", 1), "anvil").unwrap();
        assert_eq!(&r[0..2], &[0xAB, 0xCD]);
        assert_eq!(r[3], 0);
        assert_eq!(&r[6..8], &[0, 1]);
        assert_eq!(&r[r.len() - 4..], &[127, 0, 0, 1]);
    }

    #[test]
    fn aaaa_is_empty_and_foreign_is_refused() {
        let r = answer(&query("a.b.anvil", 28), "anvil").unwrap();
        assert_eq!((r[3], r[7]), (0, 0));
        let r = answer(&query("google.com", 1), "anvil").unwrap();
        assert_eq!((r[3], r[7]), (5, 0));
        assert!(answer(&[0; 5], "anvil").is_none());
    }

    #[test]
    fn a_changed_suffix_applies_to_the_running_server() {
        static TLD: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());
        *TLD.write().unwrap() = "dev".into();
        // A free port: bind, read it, release, then serve on it.
        let port = std::net::UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        std::thread::spawn(move || serve(port, &TLD));
        let client = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        client
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let rcode = |name: &str| {
            let mut buf = [0u8; 512];
            for _ in 0..20 {
                client
                    .send_to(&query(name, 1), ("127.0.0.1", port))
                    .unwrap();
                if let Ok(n) = client.recv(&mut buf) {
                    return buf[..n][3] & 0x0f;
                }
            }
            panic!("no answer from the DNS server");
        };
        assert_eq!(rcode("shop.dev"), 0);
        *TLD.write().unwrap() = "test".into();
        assert_eq!(rcode("shop.test"), 0);
        assert_eq!(rcode("shop.dev"), 5);
    }
}
