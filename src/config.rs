use anyhow::{Context, Result};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

#[derive(Clone)]
pub struct Config {
    pub root: PathBuf,
    pub dist: PathBuf,
    pub database: PathBuf,
    pub backups: PathBuf,
    pub bind: SocketAddr,
    pub trusted_proxies: Vec<IpNet>,
    pub geo_enabled: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct AdminConfig {
    pub path: String,
    pub salt: String,
    pub hash: String,
    #[serde(default)]
    pub key: String,
}

impl Config {
    pub fn load(root: PathBuf) -> Result<Self> {
        let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let port = env::var("PORT").unwrap_or_else(|_| "4173".into());
        let address: IpAddr = host.parse().context("HOST must be an IP address")?;
        let port: u16 = port.parse().context("PORT must be between 0 and 65535")?;
        let bind = SocketAddr::new(address, port);
        let trusted_proxies = env::var("TRUSTED_PROXIES")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| {
                s.trim()
                    .parse()
                    .with_context(|| format!("invalid trusted proxy CIDR: {s}"))
            })
            .collect::<Result<Vec<IpNet>>>()?;
        let geo_enabled = env::var("GEOLOOKUP_ENABLED")
            .map(|v| v == "1")
            .unwrap_or(true);
        let path = |name: &str, default: &str| -> PathBuf {
            let value = env::var_os(name)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(default));
            if value.is_absolute() {
                value
            } else {
                root.join(value)
            }
        };
        Ok(Self {
            dist: path("FROKLA_STATIC_DIR", "dist"),
            database: path("FROKLA_DB", "canvas.sqlite3"),
            backups: path("FROKLA_BACKUP_DIR", "backups"),
            root,
            bind,
            trusted_proxies,
            geo_enabled,
        })
    }

    pub fn admin(&self) -> Result<Option<AdminConfig>> {
        let path = self.root.join(".admin.json");
        if !path.exists() {
            return Ok(None);
        }
        let contents = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let config: AdminConfig =
            serde_json::from_slice(&contents).context("invalid .admin.json")?;
        if !config.path.starts_with("/manage-")
            || config.path.len() > 128
            || !config.path[1..]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || hex::decode(&config.salt).map_or(true, |salt| salt.len() != 16)
            || hex::decode(&config.hash).map_or(true, |hash| hash.len() != 32)
        {
            anyhow::bail!("invalid private workshop configuration in .admin.json");
        }
        Ok(Some(config))
    }
}
