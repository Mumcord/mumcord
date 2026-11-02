use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("database schema version {found} is newer than this app supports ({supported})")]
    SchemaTooNew { found: i64, supported: i64 },
    #[error("settings could not be (de)serialized: {0}")]
    Settings(#[from] serde_json::Error),
    #[error("secure storage error: {0}")]
    SecureStorage(#[from] keyring::Error),
    #[error("secure storage holds an incomplete identity")]
    CorruptStoredIdentity,
    #[error("secure storage could not be read earlier; restart the app to try again")]
    SecretUnavailable,
    #[error("PKCS#12 file could not be read (wrong password or unsupported file): {0}")]
    Pkcs12(#[from] p12_keystore::error::Error),
    #[error("PKCS#12 file contains no private key with a matching certificate")]
    Pkcs12NoIdentity,
    #[error("identity name must not be empty")]
    EmptyIdentityName,
    #[error("identity label must be 1 to {max} characters")]
    InvalidIdentityLabel { max: usize },
    #[error("identity {0} does not exist")]
    UnknownIdentity(i64),
    #[error("this identity is already in the list")]
    DuplicateIdentity,
    #[error("Mumcord needs at least one identity")]
    LastIdentity,
    #[error("certificate error: {0}")]
    Certificate(#[from] rcgen::Error),
    #[error("key generation failed: {0}")]
    KeyGeneration(String),
    #[error("volume {0} is outside 0–200")]
    InvalidVolume(u16),
    #[error("port 0 is not valid")]
    InvalidPort,
    #[error("relay pin needs a 40-digit hex bot hash and a 64-digit hex relay fingerprint")]
    InvalidRelayPin,
    #[error("server {0} does not exist")]
    UnknownServer(i64),
    #[error("plugin storage key must be 1 to {max} bytes")]
    InvalidPluginKey { max: usize },
    #[error("plugin storage value is larger than {max} bytes")]
    PluginValueTooLarge { max: usize },
    #[error("plugin storage quota exceeded")]
    PluginQuotaExceeded,
    #[error("at most {max} plugin repositories can be added")]
    TooManyPluginRepositories { max: usize },
    #[error("this plugin repository is already in the list")]
    DuplicatePluginRepository,
}
