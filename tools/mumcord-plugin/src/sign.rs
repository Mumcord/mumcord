//! `keygen` and `sign`: minisign keys and detached signatures, the format Mumcord verifies.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use minisign::{KeyPair, PublicKey, SecretKey, SecretKeyBox};
use mumcord_plugins::registry::{SIGNATURE_SUFFIX, key_fingerprint, normalize_key};

/// Secret keys are protected with this password when set, so CI never needs a prompt.
pub const PASSWORD_ENV: &str = "MUMCORD_PLUGIN_KEY_PASSWORD";

/// A secret key together with its public half.
pub struct Signer {
    pub public: PublicKey,
    secret: SecretKey,
}

impl Signer {
    pub fn generate() -> Result<Self> {
        let pair = KeyPair::generate_unencrypted_keypair().context("could not create a key")?;
        Ok(Self {
            public: pair.pk,
            secret: pair.sk,
        })
    }

    /// Reads a secret key file; an encrypted key is unlocked with [`PASSWORD_ENV`] or a prompt.
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("could not read the key {}", path.display()))?;
        let secret = match SecretKey::from_unencrypted_box(SecretKeyBox::from_string(&text)?) {
            Ok(secret) => secret,
            Err(_) => {
                let password = std::env::var(PASSWORD_ENV).ok();
                SecretKey::from_box(SecretKeyBox::from_string(&text)?, password)
                    .with_context(|| format!("could not unlock the key {}", path.display()))?
            }
        };
        let public = PublicKey::from_secret_key(&secret)?;
        Ok(Self { public, secret })
    }

    /// The public key as its base64 line, the form registries and manifests use.
    pub fn public_line(&self) -> String {
        self.public.to_base64()
    }

    /// A detached minisign signature in the standard text format.
    pub fn sign(&self, data: &[u8]) -> Result<String> {
        Ok(
            minisign::sign(Some(&self.public), &self.secret, data, None, None)
                .context("could not sign")?
                .into_string(),
        )
    }
}

/// Reads a public key given as a base64 line or as the path of a `.pub` file.
pub fn read_public(text_or_path: &str) -> Result<String> {
    if let Some(line) = normalize_key(text_or_path) {
        return Ok(line);
    }
    let text = fs::read_to_string(text_or_path)
        .with_context(|| format!("{text_or_path} is neither a public key nor a readable file"))?;
    normalize_key(&text).with_context(|| format!("{text_or_path} holds no minisign public key"))
}

/// Checks a detached signature with a public key line.
pub fn verify(public_line: &str, data: &[u8], signature: &str) -> Result<()> {
    let key = PublicKey::from_base64(public_line)?;
    let signature = minisign::SignatureBox::from_string(signature)?;
    minisign::verify(
        &key,
        &signature,
        std::io::Cursor::new(data),
        true,
        false,
        false,
    )
    .context("the signature does not match this key")
}

pub fn signature_path(file: &Path) -> PathBuf {
    let mut name = file.as_os_str().to_owned();
    name.push(SIGNATURE_SUFFIX);
    PathBuf::from(name)
}

pub fn keygen(out: &Path, name: &str, unencrypted: bool) -> Result<()> {
    let secret_path = out.join(format!("{name}.key"));
    let public_path = out.join(format!("{name}.pub"));
    for path in [&secret_path, &public_path] {
        if path.exists() {
            bail!(
                "{} already exists; refusing to overwrite a key",
                path.display()
            );
        }
    }
    let pair = if unencrypted {
        KeyPair::generate_unencrypted_keypair()
    } else {
        KeyPair::generate_encrypted_keypair(std::env::var(PASSWORD_ENV).ok())
    }
    .context("could not create a key")?;
    fs::create_dir_all(out)?;
    write_secret(&secret_path, &pair.sk.to_box(None)?.into_string())?;
    fs::write(&public_path, pair.pk.to_box()?.into_string())?;
    let line = pair.pk.to_base64();
    println!("public key {line}");
    println!("fingerprint {}", key_fingerprint(&line).unwrap_or_default());
    println!("secret key {} (never commit it)", secret_path.display());
    Ok(())
}

#[cfg(unix)]
fn write_secret(path: &Path, text: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_secret(path: &Path, text: &str) -> Result<()> {
    fs::write(path, text)?;
    Ok(())
}

pub fn sign_file(file: &Path, key: &Path) -> Result<()> {
    let signer = Signer::load(key)?;
    let data = fs::read(file).with_context(|| format!("could not read {}", file.display()))?;
    let path = signature_path(file);
    fs::write(&path, signer.sign(&data)?)?;
    println!("{}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keygen_sign_and_verify_round_trip() {
        let dir = tempfile::tempdir().expect("test step");
        keygen(dir.path(), "test", true).expect("test step");
        assert!(
            keygen(dir.path(), "test", true).is_err(),
            "never overwrites a key"
        );

        let file = dir.path().join("data.bin");
        fs::write(&file, b"hello").expect("test step");
        sign_file(&file, &dir.path().join("test.key")).expect("test step");

        let public =
            read_public(dir.path().join("test.pub").to_str().expect("utf-8")).expect("test step");
        let signature = fs::read_to_string(signature_path(&file)).expect("test step");
        verify(&public, b"hello", &signature).expect("valid signature");
        assert!(verify(&public, b"hellO", &signature).is_err());
        let other = Signer::generate().expect("test step").public_line();
        assert!(verify(&other, b"hello", &signature).is_err());
    }

    #[test]
    fn signatures_pass_the_client_check() {
        let signer = Signer::generate().expect("test step");
        let signature = signer.sign(b"index").expect("test step");
        let keys = mumcord_plugins::verify::RegistryKeys::pinned(&signer.public_line())
            .expect("test step");
        keys.verify(b"index", &signature)
            .expect("client accepts it");
    }
}
