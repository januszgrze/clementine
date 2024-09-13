//! # Configuration Options
//!
//! This module defines configuration options.
//!
//! This module is base for `cli` module and not dependent on it. Therefore,
//! this module can be used independently.
//!
//! ## Configuration File
//!
//! Configuration options can be read from a TOML file. File contents are
//! described in `BridgeConfig` struct.

use crate::errors::BridgeError;
use bitcoin::address::NetworkUnchecked;
use bitcoin::Network;
use serde::{Deserialize, Serialize};
use std::{fs::File, io::Read, path::PathBuf};

/// PostgreSQL database configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Database {
    /// PostgreSQL database host address.
    pub host: String,
    /// PostgreSQL database port.
    pub port: usize,
    /// PostgreSQL database user name.
    pub user: String,
    /// PostgreSQL database user password.
    pub password: String,
    /// PostgreSQL database name.
    pub name: String,
}
impl Default for Database {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 5432,
            user: "clementine".to_string(),
            password: "clementine".to_string(),
            name: "clementine".to_string(),
        }
    }
}

/// Bitcoin connection configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bitcoin {
    /// Butcoin RPC url.
    pub rpc_url: String,
    /// Bitcoin RPC user.
    pub rpc_user: String,
    /// Bitcoin RPC user password.
    pub rpc_password: String,
    /// Bitcoin network to work on.
    pub network: Network,
}
impl Default for Bitcoin {
    fn default() -> Self {
        Self {
            network: Network::Regtest,
            rpc_url: "http://127.0.0.1:18443".to_string(),
            rpc_user: "admin".to_string(),
            rpc_password: "admin".to_string(),
        }
    }
}

/// Operator configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operator {
    /// Number of operators.
    pub count: usize,
    /// Operators x-only public keys.
    pub xonly_pks: Vec<secp256k1::XOnlyPublicKey>,
    /// Number of blocks after which operator can take reimburse the bridge fund if they are honest.
    pub takes_after: u32,
    /// Operator: number of kickoff UTXOs per funding transaction.
    pub kickoff_utxos_per_tx: usize,
    /// All Secret keys. Just for testing purposes.
    pub all_secret_keys: Option<Vec<secp256k1::SecretKey>>,
}
impl Default for Operator {
    fn default() -> Self {
        Self {
            count: 3,
            xonly_pks: vec![],
            takes_after: 5,
            kickoff_utxos_per_tx: 10,
            all_secret_keys: None,
        }
    }
}

/// Verifier configuration options.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verifier {
    /// Number of verifiers.
    pub count: usize,
    /// Verifiers public keys.
    pub public_keys: Vec<secp256k1::PublicKey>,
    /// All Secret keys. Just for testing purposes.
    pub all_secret_keys: Option<Vec<secp256k1::SecretKey>>,
    /// Verifier endpoints.
    pub endpoints: Option<Vec<String>>,
}
impl Default for Verifier {
    fn default() -> Self {
        Self {
            public_keys: vec![],
            count: 7,
            all_secret_keys: None,
            endpoints: None,
        }
    }
}

/// Configuration options for any Clementine target (tests, binaries etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// PostgreSQL database configuration options.
    pub database: Database,
    /// Bitcoin connection configuration options.
    pub bitcoin: Bitcoin,
    /// Operator configuration options.
    pub operator: Operator,
    /// Verifier configuration options.
    pub verifier: Verifier,

    /// Host of the operator or the verifier
    pub host: String,
    /// Port of the operator or the verifier
    pub port: u16,
    /// Secret key for the operator or the verifier.
    pub secret_key: secp256k1::SecretKey,
    /// Operators wallet addresses.
    pub operator_wallet_addresses: Vec<bitcoin::Address<NetworkUnchecked>>,
    /// Number of operators.
    pub operator_withdrawal_fee_sats: Option<u64>,
    /// Number of blocks after which user can take deposit back if deposit request fails.
    pub user_takes_after: u32,
    /// Bridge amount in satoshis.
    pub bridge_amount_sats: u64,
    /// Threshold for confirmation.
    pub confirmation_threshold: u32,
    /// Citrea RPC URL.
    pub citrea_rpc_url: String,
    /// Bridge contract address.
    pub bridge_contract_address: String,
}
impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            database: Database::default(),
            bitcoin: Bitcoin::default(),
            operator: Operator::default(),
            verifier: Verifier::default(),

            host: "127.0.0.1".to_string(),
            port: 3030,
            secret_key: secp256k1::SecretKey::new(&mut secp256k1::rand::thread_rng()),
            user_takes_after: 5,
            bridge_amount_sats: 100_000_000,
            confirmation_threshold: 1,
            operator_withdrawal_fee_sats: None,
            operator_wallet_addresses: vec![],
            citrea_rpc_url: "http://127.0.0.1:12345".to_string(),
            bridge_contract_address: "3100000000000000000000000000000000000002".to_string(),
        }
    }
}

impl BridgeConfig {
    /// Read contents of a TOML file and generate a `BridgeConfig`.
    pub fn try_parse_file(path: PathBuf) -> Result<Self, BridgeError> {
        let mut contents = String::new();

        let mut file = match File::open(path.clone()) {
            Ok(f) => f,
            Err(e) => return Err(BridgeError::ConfigError(e.to_string())),
        };

        if let Err(e) = file.read_to_string(&mut contents) {
            return Err(BridgeError::ConfigError(e.to_string()));
        }

        tracing::trace!("Using configuration file: {:?}", path);

        BridgeConfig::try_parse_from(contents)
    }

    /// Try to parse a `BridgeConfig` from given TOML formatted string and
    /// generate a `BridgeConfig`.
    pub fn try_parse_from(input: String) -> Result<Self, BridgeError> {
        match toml::from_str::<BridgeConfig>(&input) {
            Ok(c) => Ok(c),
            Err(e) => Err(BridgeError::ConfigError(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::BridgeConfig;
    use std::{
        fs::{self, File},
        io::Write,
    };

    /// This needs a prefix for every test function, because of the async nature
    /// of the tests. I am not going to implement a mutex solution. Just do:
    /// let file_name = "someprefix".to_string() + TEST_FILE;
    pub const TEST_FILE: &str = "test.toml";

    #[test]
    fn parse_from_string() {
        // In case of a incorrect file content, we should receive an error.
        let content = "brokenfilecontent";
        match BridgeConfig::try_parse_from(content.to_string()) {
            Ok(_) => panic!("expected parse error from malformed file"),
            Err(e) => println!("{e:#?}"),
        };

        let init = BridgeConfig::default();
        match BridgeConfig::try_parse_from(toml::to_string(&init).unwrap()) {
            Ok(c) => println!("{c:#?}"),
            Err(e) => panic!("{e:#?}"),
        };
    }

    #[test]
    fn parse_from_file() {
        let file_name = "1".to_string() + TEST_FILE;
        let content = "brokenfilecontent";
        let mut file = File::create(file_name.clone()).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        match BridgeConfig::try_parse_file(file_name.clone().into()) {
            Ok(_) => panic!("expected parse error from malformed file"),
            Err(e) => println!("{e:#?}"),
        };

        // Read first example test file use for this test.
        let base_path = env!("CARGO_MANIFEST_DIR");
        let config_path = format!("{}/tests/data/test_config.toml", base_path);
        let content = fs::read_to_string(config_path).unwrap();
        let mut file = File::create(file_name.clone()).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        match BridgeConfig::try_parse_file(file_name.clone().into()) {
            Ok(c) => println!("{c:#?}"),
            Err(e) => panic!("{e:#?}"),
        };

        fs::remove_file(file_name.clone()).unwrap();
    }

    #[test]
    /// Currently, no support for headers.
    fn parse_from_file_with_headers() {
        let file_name = "2".to_string() + TEST_FILE;
        let content = "[header1]
        num_verifiers = 4

        [header2]
        confirmation_threshold = 1
        network = \"regtest\"
        bitcoin_rpc_url = \"http://localhost:18443\"
        bitcoin_rpc_user = \"admin\"
        bitcoin_rpc_password = \"admin\"\n";
        let mut file = File::create(file_name.clone()).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        match BridgeConfig::try_parse_file(file_name.clone().into()) {
            Ok(c) => println!("{c:#?}"),
            Err(e) => println!("{e:#?}"),
        };

        fs::remove_file(file_name).unwrap();
    }
}
