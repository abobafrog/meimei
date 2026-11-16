use axum::http::HeaderMap;
use hmac::{Hmac, Mac};
use ipnet::IpNet;
use sha2::Sha256;
use std::net::{IpAddr, SocketAddr};

type HmacSha256 = Hmac<Sha256>;

pub fn client_ip(peer: SocketAddr, headers: &HeaderMap, trusted: &[IpNet]) -> IpAddr {
    let direct = normalize(peer.ip());
    if !trusted.iter().any(|network| network.contains(&direct)) {
        return direct;
    }
    let Some(chain) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) else {
        return direct;
    };
    let addresses: Result<Vec<IpAddr>, _> = chain
        .split(',')
        .map(|s| s.trim().parse::<IpAddr>())
        .collect();
    let Ok(addresses) = addresses else {
        return direct;
    };
    let addresses = addresses.into_iter().map(normalize);
    let mut current = direct;
    for candidate in addresses.into_iter().rev() {
        if !trusted.iter().any(|network| network.contains(&current)) {
            break;
        }
        current = candidate;
    }
    current
}

fn normalize(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

pub fn peer_key(secret: &str, address: IpAddr) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(address.to_string().as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookies = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then(|| value.to_owned())
    })
}

pub fn random_token(bytes: usize) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use rand::{rngs::OsRng, RngCore};
    let mut value = vec![0u8; bytes];
    OsRng.fill_bytes(&mut value);
    URL_SAFE_NO_PAD.encode(value)
}

pub fn random_hex(bytes: usize) -> String {
    use rand::{rngs::OsRng, RngCore};
    let mut value = vec![0u8; bytes];
    OsRng.fill_bytes(&mut value);
    hex::encode(value)
}
