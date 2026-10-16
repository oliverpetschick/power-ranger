//! Captive-portal DNS: every A query resolves to the AP's own IP.

const TTL_S: u32 = 60;

/// Reply to one DNS query packet, `None` if it isn't a plain single-question query.
pub fn reply(q: &[u8], ip: [u8; 4]) -> Option<Vec<u8>> {
    if q.len() < 12 || q[2] & 0xF8 != 0 || q[4..6] != [0, 1] {
        return None; // too short, a response, non-standard opcode, or qdcount != 1
    }
    let mut i = 12;
    loop {
        let len = *q.get(i)? as usize;
        if len & 0xC0 != 0 {
            return None; // compression in a question: not from a sane client
        }
        i += 1 + len;
        if len == 0 {
            break;
        }
    }
    let qend = i + 4;
    let qtype = q.get(i..qend).map(|t| u16::from_be_bytes([t[0], t[1]]))?;
    let answer = qtype == 1 || qtype == 255; // A or ANY; others get NOERROR, no data

    let mut r = Vec::with_capacity(qend + 16);
    r.extend_from_slice(&q[..2]); // id
    r.extend_from_slice(&[0x84 | (q[2] & 0x01), 0x00]); // QR, AA, copy RD; rcode 0
    r.extend_from_slice(&[0, 1, 0, answer as u8, 0, 0, 0, 0]); // qd, an, ns, ar
    r.extend_from_slice(&q[12..qend]);
    if answer {
        r.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1]); // name ptr, type A, class IN
        r.extend_from_slice(&TTL_S.to_be_bytes());
        r.extend_from_slice(&[0, 4]);
        r.extend_from_slice(&ip);
    }
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    const IP: [u8; 4] = [192, 168, 71, 1];

    fn query(qtype: u16) -> Vec<u8> {
        let mut q = vec![0xAB, 0xCD, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for label in ["connectivitycheck", "gstatic", "com"] {
            q.push(label.len() as u8);
            q.extend_from_slice(label.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&[0, 1]);
        q
    }

    #[test]
    fn a_query_points_to_ap() {
        let q = query(1);
        let r = reply(&q, IP).unwrap();
        assert_eq!(&r[..2], &[0xAB, 0xCD]);
        assert_eq!(r[2], 0x85); // QR | AA | RD
        assert_eq!(&r[6..8], &[0, 1]); // one answer
        assert_eq!(&r[12..q.len()], &q[12..]);
        assert_eq!(&r[r.len() - 4..], &IP);
    }

    #[test]
    fn aaaa_gets_empty_answer() {
        let r = reply(&query(28), IP).unwrap();
        assert_eq!(&r[6..8], &[0, 0]);
        assert_eq!(r.len(), query(28).len());
    }

    #[test]
    fn garbage_is_dropped() {
        let q = query(1);
        assert!(reply(&q[..q.len() - 1], IP).is_none()); // truncated qtype/class
        assert!(reply(&q[..5], IP).is_none());
        let mut resp = q.clone();
        resp[2] |= 0x80;
        assert!(reply(&resp, IP).is_none());
        let mut two = q.clone();
        two[5] = 2;
        assert!(reply(&two, IP).is_none());
        let mut ptr = q;
        ptr[12] = 0xC0;
        assert!(reply(&ptr, IP).is_none());
    }
}
