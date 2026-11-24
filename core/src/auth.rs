//! PIN login with an offline recovery key.
//!
//! * The PIN and the recovery key are both stored only as Argon2id hashes.
//! * Failed attempts (PIN or recovery key) trigger an escalating lockout that
//!   survives app restarts because it lives in the database.
//! * Using the recovery key to reset the PIN rotates the key, so a key that has
//!   been typed somewhere is never valid twice.

use argon2::Argon2;
use argon2::password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash};
use rusqlite::Connection;

use crate::db::{get_setting, set_setting};
use crate::error::{CoreError, Result, invalid};

pub const PIN_LENGTH: usize = 6;

const KEY_PIN_HASH: &str = "auth.pin_hash";
const KEY_RECOVERY_HASH: &str = "auth.recovery_hash";
const KEY_FAILED: &str = "auth.failed_attempts";
const KEY_LOCKED_UNTIL: &str = "auth.locked_until";

/// Free attempts before lockouts start.
const FREE_ATTEMPTS: i64 = 5;
/// First lockout length; doubles with every further failure, capped below.
const BASE_LOCK_SECS: i64 = 30;
const MAX_LOCK_SECS: i64 = 15 * 60;

/// Crockford base32 – no I, L, O, U, so keys are easy to read and type.
const KEY_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const KEY_GROUPS: usize = 5;
const KEY_GROUP_LEN: usize = 5;

pub fn is_initialized(conn: &Connection) -> Result<bool> {
    Ok(get_setting(conn, KEY_PIN_HASH)?.is_some())
}

/// First-run setup. Returns the recovery key, which must be shown to the user
/// exactly once – it cannot be recovered later.
pub fn setup(conn: &Connection, pin: &str, confirm: &str) -> Result<String> {
    if is_initialized(conn)? {
        return Err(CoreError::Auth("A PIN has already been set up.".into()));
    }
    validate_new_pin(pin, confirm)?;
    let key = generate_recovery_key()?;
    let tx = conn.unchecked_transaction()?;
    set_setting(&tx, KEY_PIN_HASH, &hash(pin)?)?;
    set_setting(&tx, KEY_RECOVERY_HASH, &hash(&normalize_recovery_key(&key))?)?;
    reset_failures(&tx)?;
    tx.commit()?;
    Ok(key)
}

pub fn verify_pin(conn: &Connection, pin: &str) -> Result<()> {
    check_lockout(conn)?;
    let stored = get_setting(conn, KEY_PIN_HASH)?
        .ok_or_else(|| CoreError::Auth("No PIN has been set up yet.".into()))?;
    if verify(pin, &stored)? {
        reset_failures(conn)?;
        Ok(())
    } else {
        Err(register_failure(conn, "Incorrect PIN.")?)
    }
}

/// Checks a recovery key without changing anything, so a UI can confirm the
/// key before asking for a new PIN. Failures count towards the lockout.
pub fn verify_recovery_key(conn: &Connection, recovery_key: &str) -> Result<()> {
    check_lockout(conn)?;
    let stored = get_setting(conn, KEY_RECOVERY_HASH)?
        .ok_or_else(|| CoreError::Auth("No recovery key has been set up.".into()))?;
    let normalized = normalize_recovery_key(recovery_key);
    if normalized.len() == KEY_GROUPS * KEY_GROUP_LEN && verify(&normalized, &stored)? {
        Ok(())
    } else {
        Err(register_failure(conn, "That recovery key is not valid.")?)
    }
}

/// Resets the PIN using the recovery key. Returns a *new* recovery key.
pub fn reset_with_recovery_key(
    conn: &Connection,
    recovery_key: &str,
    new_pin: &str,
    confirm: &str,
) -> Result<String> {
    check_lockout(conn)?;
    validate_new_pin(new_pin, confirm)?;
    let stored = get_setting(conn, KEY_RECOVERY_HASH)?
        .ok_or_else(|| CoreError::Auth("No recovery key has been set up.".into()))?;
    let normalized = normalize_recovery_key(recovery_key);
    if normalized.len() != KEY_GROUPS * KEY_GROUP_LEN || !verify(&normalized, &stored)? {
        return Err(register_failure(conn, "That recovery key is not valid.")?);
    }
    let new_key = generate_recovery_key()?;
    let tx = conn.unchecked_transaction()?;
    set_setting(&tx, KEY_PIN_HASH, &hash(new_pin)?)?;
    set_setting(&tx, KEY_RECOVERY_HASH, &hash(&normalize_recovery_key(&new_key))?)?;
    reset_failures(&tx)?;
    tx.commit()?;
    Ok(new_key)
}

pub fn change_pin(conn: &Connection, current: &str, new_pin: &str, confirm: &str) -> Result<()> {
    validate_new_pin(new_pin, confirm)?;
    verify_pin(conn, current)?;
    set_setting(conn, KEY_PIN_HASH, &hash(new_pin)?)
}

/// Issues a fresh recovery key (e.g. if the old one was lost). Requires the PIN.
pub fn regenerate_recovery_key(conn: &Connection, pin: &str) -> Result<String> {
    verify_pin(conn, pin)?;
    let key = generate_recovery_key()?;
    set_setting(conn, KEY_RECOVERY_HASH, &hash(&normalize_recovery_key(&key))?)?;
    Ok(key)
}

/// Seconds left on the current lockout, or 0.
pub fn lockout_remaining(conn: &Connection) -> Result<i64> {
    let until: i64 = get_setting(conn, KEY_LOCKED_UNTIL)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    Ok((until - now_unix()).max(0))
}

pub fn validate_new_pin(pin: &str, confirm: &str) -> Result<()> {
    if pin.len() != PIN_LENGTH || !pin.bytes().all(|b| b.is_ascii_digit()) {
        return invalid(format!("PIN must be exactly {PIN_LENGTH} digits."));
    }
    let d: Vec<i16> = pin.bytes().map(|b| (b - b'0') as i16).collect();
    if d.iter().all(|&x| x == d[0]) {
        return invalid("PIN can't be a single repeated digit.");
    }
    let step = d[1] - d[0];
    if step.abs() == 1 && d.windows(2).all(|w| w[1] - w[0] == step) {
        return invalid("PIN can't be a simple sequence like 123456.");
    }
    if pin != confirm {
        return invalid("The two PINs don't match.");
    }
    Ok(())
}

/// Uppercases, drops separators and maps look-alike characters so users can
/// type the key loosely.
pub fn normalize_recovery_key(key: &str) -> String {
    key.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        })
        .collect()
}

fn generate_recovery_key() -> Result<String> {
    let mut bytes = [0u8; KEY_GROUPS * KEY_GROUP_LEN];
    getrandom::fill(&mut bytes).map_err(|e| CoreError::Crypto(e.to_string()))?;
    let chars: Vec<char> = bytes
        .iter()
        .map(|b| KEY_ALPHABET[(*b as usize) % KEY_ALPHABET.len()] as char)
        .collect();
    Ok(chars
        .chunks(KEY_GROUP_LEN)
        .map(|c| c.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join("-"))
}

fn hash(secret: &str) -> Result<String> {
    Argon2::default()
        .hash_password(secret.as_bytes())
        .map(|h| h.to_string())
        .map_err(|e| CoreError::Crypto(e.to_string()))
}

fn verify(secret: &str, phc: &str) -> Result<bool> {
    let parsed = PasswordHash::new(phc).map_err(|e| CoreError::Crypto(e.to_string()))?;
    Ok(Argon2::default()
        .verify_password(secret.as_bytes(), &parsed)
        .is_ok())
}

fn check_lockout(conn: &Connection) -> Result<()> {
    match lockout_remaining(conn)? {
        0 => Ok(()),
        secs => Err(CoreError::LockedOut(secs)),
    }
}

fn register_failure(conn: &Connection, msg: &str) -> Result<CoreError> {
    let failed: i64 = get_setting(conn, KEY_FAILED)?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
        + 1;
    set_setting(conn, KEY_FAILED, &failed.to_string())?;
    if failed >= FREE_ATTEMPTS {
        let exp = (failed - FREE_ATTEMPTS).min(10) as u32;
        let secs = (BASE_LOCK_SECS * 2i64.pow(exp)).min(MAX_LOCK_SECS);
        set_setting(conn, KEY_LOCKED_UNTIL, &(now_unix() + secs).to_string())?;
        return Ok(CoreError::LockedOut(secs));
    }
    let left = FREE_ATTEMPTS - failed;
    Ok(CoreError::Auth(format!(
        "{msg} {left} attempt{} left before a temporary lock.",
        if left == 1 { "" } else { "s" }
    )))
}

fn reset_failures(conn: &Connection) -> Result<()> {
    set_setting(conn, KEY_FAILED, "0")?;
    set_setting(conn, KEY_LOCKED_UNTIL, "0")
}

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}
