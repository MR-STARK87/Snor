//! The single-file setup bundle.
//!
//! `SnorSetup.exe` is three things end to end — the setup binary, the app's
//! bytes, and a footer:
//!
//! ```text
//! [ setup.exe ][ snor.exe ][ magic 8 | version 4 | len 8 | fnv 8 ]
//! ```
//!
//! The footer sits at the very end of the file and is found by reading back
//! from it, so nothing has to parse PE headers or a manifest to locate the
//! payload — and the setup binary itself is the first `len - 28 - payload`
//! bytes, which is where the installed `uninstall.exe` is copied from (an
//! uninstaller has no reason to carry an 11 MB app around).
//!
//! The packer truncates any bundle already present before appending, so
//! packing twice does not stack two payloads. A running setup prefers its own
//! appended payload and falls back to an `snor.exe` beside the setup binary,
//! which keeps `cargo run -p snor-installer --bin snor-setup` working without
//! a repack every time. `Launch::detect` owns the priority order between the
//! two, because the uninstaller's own directory also has an `snor.exe` in it.

use std::path::{Path, PathBuf};

/// Everything after the magic: version(4) + payload length(8) + checksum(8).
pub const FOOTER_LEN: u64 = 28;

/// The `1` is the format version. A future layout bumps it and old readers
/// refuse the file rather than mis-parsing it.
const MAGIC: [u8; 8] = *b"SNORPKG1";
const FORMAT_VERSION: u32 = 1;

/// The app, and where the setup binary stops being the setup binary.
#[derive(Debug)]
pub struct Bundle {
    /// The app's bytes, exactly as they will be written to disk.
    pub exe: Vec<u8>,
    /// Length of the setup binary itself — everything before the payload.
    /// The source of the installed `uninstall.exe`.
    pub prefix_len: u64,
}

/// FNV-1a, 64 bit. Not cryptographic, and it does not pretend to be: what it
/// has to catch is a truncated or corrupted download, so the installer can
/// say "get a fresh copy" instead of writing a broken `snor.exe`.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn footer(payload_len: u64, fnv: u64) -> [u8; FOOTER_LEN as usize] {
    let mut out = [0u8; FOOTER_LEN as usize];
    out[..8].copy_from_slice(&MAGIC);
    out[8..12].copy_from_slice(&FORMAT_VERSION.to_le_bytes());
    out[12..20].copy_from_slice(&payload_len.to_le_bytes());
    out[20..28].copy_from_slice(&fnv.to_le_bytes());
    out
}

/// Parse a setup file's bytes.
///
/// `Ok(None)` when the file carries no footer at all — a bare setup binary, or
/// any other exe. `Err` when a footer is there but the file does not check
/// out: that is a bundle that was damaged, and quietly treating it as "no
/// payload" would send the user down the sidecar path and the wrong error.
pub fn parse(file: &[u8]) -> Result<Option<Bundle>, String> {
    let len = file.len() as u64;
    if len < FOOTER_LEN {
        return Ok(None);
    }
    let tail = &file[(len - FOOTER_LEN) as usize..];
    if tail[..8] != MAGIC {
        return Ok(None);
    }
    let version = u32::from_le_bytes(tail[8..12].try_into().expect("4 bytes"));
    if version != FORMAT_VERSION {
        return Err(format!(
            "this setup file uses bundle format {version}, but this binary only understands \
             format {FORMAT_VERSION} — download the latest setup"
        ));
    }
    let payload_len = u64::from_le_bytes(tail[12..20].try_into().expect("8 bytes"));
    let fnv = u64::from_le_bytes(tail[20..28].try_into().expect("8 bytes"));
    // `FOOTER_LEN + payload_len` can overflow on its own. `payload_len` is read
    // from the file, so a crafted footer can carry `u64::MAX` and the addition
    // panics in a debug build (and silently wraps in a release one) before
    // `checked_sub` is ever reached. Both steps are checked, in order.
    let payload_start = FOOTER_LEN
        .checked_add(payload_len)
        .and_then(|end| len.checked_sub(end))
        .ok_or_else(|| "this setup file is truncated — download a fresh copy".to_string())?;
    let payload = &file[payload_start as usize..(len - FOOTER_LEN) as usize];
    if fnv1a(payload) != fnv {
        return Err(
            "this setup file is corrupted (its checksum does not match) — download a fresh copy"
                .to_string(),
        );
    }
    Ok(Some(Bundle {
        exe: payload.to_vec(),
        prefix_len: payload_start,
    }))
}
/// Read a setup file and parse it. Reading it whole is fine: the setup binary
/// plus the app is ~20 MB at this point in the project's life.
pub fn parse_file(path: &Path) -> Result<Option<Bundle>, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    parse(&bytes)
}

/// An `snor.exe` sitting beside `path`, if there is one — the dev fallback
/// that makes `cargo run --bin snor-setup` work without a repack.
pub fn sidecar(path: &Path) -> Option<PathBuf> {
    let candidate = path.with_file_name("snor.exe");
    candidate.is_file().then_some(candidate)
}

/// Append `app` to the setup binary at `setup`, writing the distributable to
/// `out`. Strips any bundle already there first, so packing is idempotent.
/// Returns the size of the file that was written.
pub fn bundle(setup: &Path, app: &[u8], out: &Path) -> Result<u64, String> {
    let mut bytes =
        std::fs::read(setup).map_err(|e| format!("could not read {}: {e}", setup.display()))?;
    if let Some(existing) = parse(&bytes)? {
        bytes.truncate(existing.prefix_len as usize);
    }
    bytes.extend_from_slice(app);
    bytes.extend_from_slice(&footer(app.len() as u64, fnv1a(app)));
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    }
    std::fs::write(out, &bytes).map_err(|e| format!("could not write {}: {e}", out.display()))?;
    Ok(bytes.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A setup binary with a payload bundled the way `bundle()` does it.
    fn bundled(prefix: &[u8], app: &[u8]) -> Vec<u8> {
        let mut bytes = prefix.to_vec();
        bytes.extend_from_slice(app);
        bytes.extend_from_slice(&footer(app.len() as u64, fnv1a(app)));
        bytes
    }

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("snor-installer-{}-{name}", std::process::id()))
    }

    #[test]
    fn fnv1a_matches_the_reference_vectors() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn a_bundled_payload_reads_back_byte_for_byte() {
        let setup = b"pretend setup binary".as_slice();
        let app = b"pretend snor.exe".as_slice();
        let found = parse(&bundled(setup, app)).expect("parse").expect("a bundle");
        assert_eq!(found.exe, app);
        assert_eq!(found.prefix_len, setup.len() as u64);
    }

    #[test]
    fn a_plain_binary_carries_no_payload() {
        assert!(parse(b"an exe with no footer at all").expect("parse").is_none());
    }

    /// A damaged bundle must not be silently read as "no payload": that would
    /// send the user down the sidecar path and report "this file does not
    /// contain Snor" instead of telling them the download is broken.
    #[test]
    fn a_corrupted_payload_is_refused_rather_than_written() {
        let mut file = bundled(b"setup", b"the app");
        let victim = file.len() - FOOTER_LEN as usize - 1;
        file[victim] ^= 0xFF;
        let error = parse(&file).expect_err("corruption must be an error");
        assert!(error.contains("corrupted"), "unexpected message: {error}");
    }

    /// A footer that promises more payload than the file holds is truncation,
    /// and reads as its own error rather than as corruption.
    #[test]
    fn a_footer_claiming_missing_bytes_is_truncation() {
        let app = b"the app";
        let mut file = b"setup".to_vec();
        file.extend_from_slice(app);
        file.extend_from_slice(&footer(app.len() as u64 + 100, fnv1a(app)));
        let error = parse(&file).expect_err("truncation must be an error");
        assert!(error.contains("truncated"), "unexpected message: {error}");
    }

    /// A footer claiming an impossible payload length is refused, not added.
    ///
    /// `FOOTER_LEN + payload_len` used to overflow here: `payload_len` comes from
    /// the file, so `u64::MAX` is a value the file controls, and the addition
    /// panicked in a debug build before the length check could refuse it.
    #[test]
    fn a_footer_claiming_an_impossible_length_is_refused() {
        let app = b"the app";
        let mut file = b"setup".to_vec();
        file.extend_from_slice(app);
        file.extend_from_slice(&footer(u64::MAX, fnv1a(app)));
        let error = parse(&file).expect_err("an impossible length must be an error");
        assert!(error.contains("truncated"), "unexpected message: {error}");
    }

    /// Packing a file that is already a bundle replaces the payload instead of
    /// stacking a second one, and leaves the original setup binary below it.
    #[test]
    fn packing_strips_an_older_bundle_instead_of_stacking_two() {
        let setup_path = temp_path("setup.exe");
        let first = temp_path("out1.exe");
        let second = temp_path("out2.exe");
        std::fs::write(&setup_path, b"the setup binary").expect("write setup");
        bundle(&setup_path, b"app one", &first).expect("first pack");
        bundle(&first, b"app two", &second).expect("second pack");
        let found = parse_file(&second).expect("parse").expect("a bundle");
        assert_eq!(found.exe, b"app two");
        let bytes = std::fs::read(&second).expect("read");
        assert_eq!(&bytes[..found.prefix_len as usize], b"the setup binary");
        for path in [setup_path, first, second] {
            let _ = std::fs::remove_file(path);
        }
    }
}