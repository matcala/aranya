//! Utilities for loading environment variables for the example.

use std::{env, path::Path, str::FromStr};

use age::secrecy::{ExposeSecret, SecretString};
use anyhow::{Context, Result};
use aranya_daemon_api::Role;
use aranya_util::Addr;
use tokio::fs;

/// Environment variable name constants.
const LOG_LEVEL_ENV_VAR: &str = "ARANYA_EXAMPLE";
const ONBOARDING_PASSPHRASE_ENV_VAR: &str = "ARANYA_ONBOARDING_PASSPHRASE";
const AQC_ADDR_ENV_VAR: &str = "ARANYA_AQC_ADDR";
const TCP_ADDR_ENV_VAR: &str = "ARANYA_TCP_ADDR";
const SYNC_ADDR_ENV_VAR: &str = "ARANYA_SYNC_ADDR";
const COSMOS_CMD_LISTEN_ADDR_ENV_VAR: &str = "COSMOS_CMD_LISTEN_ADDR";
const COSMOS_TLM_SEND_ADDR_ENV_VAR: &str = "COSMOS_TLM_SEND_ADDR";
const TARGET_CMD_SEND_ADDR_ENV_VAR: &str = "TARGET_CMD_SEND_ADDR";
const TARGET_TLM_LISTEN_ADDR_ENV_VAR: &str = "TARGET_TLM_LISTEN_ADDR";

/// Default environment variables.
const DEFAULT_ENV_VARS: ConstEnvVars<'_> = ConstEnvVars {
    level: "info",
    passphrase: "passphrase",
    owner: ConstDevice {
        name: "owner",
        aqc_addr: "127.0.0.1:10000",
        tcp_addr: "127.0.0.1:10001",
        sync_addr: "127.0.0.1:10002",
        role: Role::Owner,
        udp_listen_addr: None,
        udp_target_addr: None,
    },
    admin: ConstDevice {
        name: "admin",
        aqc_addr: "127.0.0.1:10003",
        tcp_addr: "127.0.0.1:10004",
        sync_addr: "127.0.0.1:10005",
        role: Role::Admin,
        udp_listen_addr: None,
        udp_target_addr: None,
    },
    operator: ConstDevice {
        name: "operator",
        aqc_addr: "127.0.0.1:10006",
        tcp_addr: "127.0.0.1:10007",
        sync_addr: "127.0.0.1:10008",
        role: Role::Operator,
        udp_listen_addr: None,
        udp_target_addr: None,
    },
    membera: ConstDevice {
        name: "membera",
        aqc_addr: "127.0.0.1:10009",
        tcp_addr: "127.0.0.1:10010",
        sync_addr: "127.0.0.1:10011",
        role: Role::Member,
        udp_listen_addr: Some("127.0.0.1:8001"),
        udp_target_addr: Some("127.0.0.1:9001"),
    },
    memberb: ConstDevice {
        name: "memberb",
        aqc_addr: "127.0.0.1:10012",
        tcp_addr: "127.0.0.1:10013",
        sync_addr: "127.0.0.1:10014",
        role: Role::Member,
        udp_listen_addr: Some("127.0.0.1:8002"),
        udp_target_addr: Some("127.0.0.1:9002"),
    },
};

const DEVICE_LIST: [(&str, Role); 5] = [
    ("owner", Role::Owner),
    ("admin", Role::Admin),
    ("operator", Role::Operator),
    ("membera", Role::Member),
    ("memberb", Role::Member),
];

/// Environment variables.
#[derive(Debug)]
pub struct EnvVars {
    /// Tracing log level.
    pub level: String,
    /// Onboarding passphrase for encrypting team info with `age`.
    pub passphrase: SecretString,
    /// Owner device
    pub owner: Device,
    /// Admin device
    pub admin: Device,
    /// Operator device
    pub operator: Device,
    /// Member A device
    pub membera: Device,
    /// Member B device
    pub memberb: Device,
}

impl EnvVars {
    /// Load device info from environment variables.
    pub fn load() -> Result<Self> {
        let level = env_var(LOG_LEVEL_ENV_VAR)?;
        let passphrase = SecretString::from(env_var::<String>(ONBOARDING_PASSPHRASE_ENV_VAR)?);
        let mut devices = Vec::new();
        for device in DEVICE_LIST {
            let udp_listen_addr = env_var::<Addr>(&format!("ARANYA_UDP_LISTEN_ADDR_{}", device.0.to_uppercase())).ok();
            let udp_target_addr = env_var::<Addr>(&format!("ARANYA_UDP_TARGET_ADDR_{}", device.0.to_uppercase())).ok();
            
            let device = Device {
                name: device.0.to_string(),
                aqc_addr: env_var(&format!("ARANYA_AQC_ADDR_{}", device.0.to_uppercase()))?,
                tcp_addr: env_var(&format!("ARANYA_TCP_ADDR_{}", device.0.to_uppercase()))?,
                sync_addr: env_var(&format!("ARANYA_SYNC_ADDR_{}", device.0.to_uppercase()))?,
                role: device.1,
                udp_listen_addr,
                udp_target_addr,
            };
            devices.push(device);
        }
        let memberb = devices.pop().expect("expected device");
        let membera = devices.pop().expect("expected device");
        let operator = devices.pop().expect("expected device");
        let admin = devices.pop().expect("expected device");
        let owner = devices.pop().expect("expected device");
        Ok(Self {
            level,
            passphrase,
            owner,
            admin,
            operator,
            membera,
            memberb,
        })
    }

    /// Generate environment file.
    pub async fn generate(&self, path: &Path) -> Result<()> {
        let mut buf = "".to_string();
        buf += &format!("export {}={}\r\n", LOG_LEVEL_ENV_VAR, self.level);
        buf += &format!(
            "export {}={}\r\n",
            ONBOARDING_PASSPHRASE_ENV_VAR,
            self.passphrase.expose_secret()
        );
        for device in self.devices() {
            buf += &format!(
                "export {}_{}={}\r\n",
                SYNC_ADDR_ENV_VAR,
                device.name.to_uppercase(),
                device.sync_addr
            );
            buf += &format!(
                "export {}_{}={}\r\n",
                AQC_ADDR_ENV_VAR,
                device.name.to_uppercase(),
                device.aqc_addr
            );
            buf += &format!(
                "export {}_{}={}\r\n",
                TCP_ADDR_ENV_VAR,
                device.name.to_uppercase(),
                device.tcp_addr
            );
            if let Some(udp_listen_addr) = &device.udp_listen_addr {
                buf += &format!(
                    "export ARANYA_UDP_LISTEN_ADDR_{}={}\r\n",
                    device.name.to_uppercase(),
                    udp_listen_addr
                );
            }
            if let Some(udp_target_addr) = &device.udp_target_addr {
                buf += &format!(
                    "export ARANYA_UDP_TARGET_ADDR_{}={}\r\n",
                    device.name.to_uppercase(),
                    udp_target_addr
                );
            }
        }
        fs::write(path, buf).await?;
        Ok(())
    }

    /// Set environment variables.
    pub fn set(&self) {
        env::set_var("ARANYA_EXAMPLE", self.level.clone());
        env::set_var(
            ONBOARDING_PASSPHRASE_ENV_VAR,
            self.passphrase.expose_secret(),
        );
        for device in self.devices() {
            env::set_var(
                format!("{}_{}", SYNC_ADDR_ENV_VAR, device.name.to_uppercase()),
                device.sync_addr.to_string(),
            );
            env::set_var(
                format!("{}_{}", AQC_ADDR_ENV_VAR, device.name.to_uppercase()),
                device.aqc_addr.to_string(),
            );
            env::set_var(
                format!("{}_{}", TCP_ADDR_ENV_VAR, device.name.to_uppercase()),
                device.tcp_addr.to_string(),
            );
            if let Some(udp_listen_addr) = &device.udp_listen_addr {
                env::set_var(
                    format!("ARANYA_UDP_LISTEN_ADDR_{}", device.name.to_uppercase()),
                    udp_listen_addr.to_string(),
                );
            }
            if let Some(udp_target_addr) = &device.udp_target_addr {
                env::set_var(
                    format!("ARANYA_UDP_TARGET_ADDR_{}", device.name.to_uppercase()),
                    udp_target_addr.to_string(),
                );
            }
        }
    }

    /// Return an Iterator to the list of devices.
    pub fn devices(&self) -> impl Iterator<Item = &Device> {
        vec![
            &self.owner,
            &self.admin,
            &self.operator,
            &self.membera,
            &self.memberb,
        ]
        .into_iter()
    }
}

impl Default for EnvVars {
    fn default() -> Self {
        DEFAULT_ENV_VARS.into()
    }
}

/// Constant representation of environment variables.
#[derive(Debug)]
struct ConstEnvVars<'a> {
    /// Tracing log level.
    level: &'a str,
    /// Onboarding passphrase for encrypting team info with `age`.
    passphrase: &'a str,
    /// Owner device
    owner: ConstDevice<'a>,
    /// Admin device
    admin: ConstDevice<'a>,
    /// Operator device
    operator: ConstDevice<'a>,
    /// Member A device
    membera: ConstDevice<'a>,
    /// Member B device
    memberb: ConstDevice<'a>,
}

impl From<ConstEnvVars<'_>> for EnvVars {
    fn from(value: ConstEnvVars<'_>) -> Self {
        EnvVars {
            level: value.level.into(),
            passphrase: value.passphrase.into(),
            owner: value.owner.into(),
            admin: value.admin.into(),
            operator: value.operator.into(),
            membera: value.membera.into(),
            memberb: value.memberb.into(),
        }
    }
}

/// Constant representation of an Aranya device.
#[derive(Debug)]
struct ConstDevice<'a> {
    name: &'a str,
    aqc_addr: &'a str,
    tcp_addr: &'a str,
    sync_addr: &'a str,
    role: Role,
    udp_listen_addr: Option<&'a str>,
    udp_target_addr: Option<&'a str>,
}

/// Aranya device info.
#[derive(Clone, Debug)]
pub struct Device {
    /// Device name.
    pub name: String,
    /// AQC address.
    pub aqc_addr: Addr,
    /// TCP address.
    pub tcp_addr: Addr,
    /// Sync address.
    pub sync_addr: Addr,
    /// Device's role.
    pub role: Role,
    /// UDP listen address for members (not required for others).
    pub udp_listen_addr: Option<Addr>,
    /// UDP target address for members (not required for others).
    pub udp_target_addr: Option<Addr>,
}

impl From<ConstDevice<'_>> for Device {
    fn from(value: ConstDevice<'_>) -> Self {
        Device {
            name: value.name.to_string(),
            aqc_addr: Addr::from_str(value.aqc_addr).expect("expected addr"),
            tcp_addr: Addr::from_str(value.tcp_addr).expect("expected addr"),
            sync_addr: Addr::from_str(value.sync_addr).expect("expected addr"),
            role: value.role,
            udp_listen_addr: value.udp_listen_addr.map(|addr| Addr::from_str(addr).expect("expected UDP addr")),
            udp_target_addr: value.udp_target_addr.map(|addr| Addr::from_str(addr).expect("expected UDP addr")),
        }
    }
}

/// Parses an environment variable, including the name in the error.
fn env_var<T>(name: &str) -> Result<T>
where
    T: FromStr<Err: core::error::Error + Send + Sync + 'static>,
{
    (|| -> Result<T> { Ok(env::var(name)?.parse()?) })().with_context(|| format!("bad `{name}`"))
}
