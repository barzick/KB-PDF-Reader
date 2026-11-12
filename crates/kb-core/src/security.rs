//! Password protection: opening protected PDFs, removing and adding a password.
//!
//! Protected documents are decrypted in memory (PDFium, which reads every
//! security handler) so that all tools work on them; the protection is put
//! back (AES-256) when the document is saved.

use std::collections::BTreeMap;
use std::os::raw::{c_int, c_ulong, c_void};
use std::sync::Arc;

use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
use lopdf::encryption::{EncryptionState, EncryptionVersion, Permissions};
use lopdf::Document;
use pdfium_render::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{pdfium, KbError, Result};

fn err(e: impl std::fmt::Display) -> KbError {
    KbError::Pdf(e.to_string())
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Protection {
    /// Not encrypted.
    None,
    /// Encrypted but opens without a password (restrictions only).
    Restricted,
    /// A password is needed to open it.
    Password,
}

fn has_encrypt(bytes: &[u8]) -> bool {
    // "/Encrypt" as a key (not "/EncryptMetadata" of a leftover dictionary).
    bytes.windows(9).any(|w| &w[..8] == b"/Encrypt" && !w[8].is_ascii_alphanumeric())
}

pub fn protection(bytes: &[u8]) -> Protection {
    if !has_encrypt(bytes) {
        return Protection::None;
    }
    let Ok(p) = pdfium() else { return Protection::None };
    match p.load_pdf_from_byte_slice(bytes, None) {
        Ok(_) => Protection::Restricted,
        Err(PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError)) => Protection::Password,
        Err(_) => Protection::None,
    }
}

/// What the author allowed (kept when the document is saved again).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProtectOptions {
    /// Needed to open the document (empty: restrictions only).
    pub user_password: String,
    /// Needed to change the restrictions; random when empty.
    #[serde(default)]
    pub owner_password: String,
    #[serde(default = "yes")]
    pub allow_print: bool,
    #[serde(default = "yes")]
    pub allow_copy: bool,
    #[serde(default = "yes")]
    pub allow_modify: bool,
}

fn yes() -> bool {
    true
}

#[repr(C)]
struct Sink {
    base: FPDF_FILEWRITE,
    out: Vec<u8>,
}

unsafe extern "C" fn write_block(this: *mut FPDF_FILEWRITE, data: *const c_void, size: c_ulong) -> c_int {
    // SAFETY: `this` is the first field of a live `Sink` (repr C).
    let sink = unsafe { &mut *(this as *mut Sink) };
    if size > 0 && !data.is_null() {
        sink.out.extend_from_slice(unsafe { std::slice::from_raw_parts(data as *const u8, size as usize) });
    }
    1
}

/// A decrypted copy, and the protection it had.
pub fn unlock(bytes: &[u8], password: &str) -> Result<(Vec<u8>, ProtectOptions)> {
    let p = pdfium()?;
    let b = p.bindings();
    let pw = (!password.is_empty()).then_some(password);
    // SAFETY: plain PDFium C calls; the document is closed before returning and
    // `bytes` outlives it.
    unsafe {
        let doc = b.FPDF_LoadMemDocument(bytes, pw);
        if doc.is_null() {
            return Err(match b.FPDF_GetLastError() {
                4 => KbError::Pdf("mot de passe incorrect".into()),
                _ => KbError::Pdf("document illisible".into()),
            });
        }
        let perms = b.FPDF_GetDocPermissions(doc) as u32;
        let mut sink = Sink { base: FPDF_FILEWRITE { version: 1, WriteBlock: Some(write_block) }, out: vec![] };
        const FPDF_REMOVE_SECURITY: u32 = 3;
        let ok = b.FPDF_SaveAsCopy(doc, &mut sink.base, FPDF_REMOVE_SECURITY as _);
        b.FPDF_CloseDocument(doc);
        if ok == 0 || sink.out.is_empty() {
            return Err(err("impossible de déverrouiller le document"));
        }
        let opts = ProtectOptions {
            user_password: password.to_string(),
            owner_password: String::new(),
            allow_print: perms & (1 << 2) != 0,
            allow_copy: perms & (1 << 4) != 0,
            allow_modify: perms & (1 << 3) != 0,
        };
        Ok((sink.out, opts))
    }
}

/// Encrypts the document with AES-256.
pub fn protect(bytes: &[u8], o: &ProtectOptions) -> Result<Vec<u8>> {
    let mut doc = Document::load_mem(bytes).map_err(err)?;
    if doc.trailer.has(b"Encrypt") {
        return Err(err("le document est déjà protégé"));
    }
    let mut key = [0u8; 32];
    getrandom::fill(&mut key).map_err(err)?;
    let owner = if o.owner_password.is_empty() {
        let mut r = [0u8; 16];
        getrandom::fill(&mut r).map_err(err)?;
        r.iter().map(|b| format!("{b:02x}")).collect()
    } else {
        o.owner_password.clone()
    };
    let mut perms = Permissions::all();
    if !o.allow_print {
        perms.remove(Permissions::PRINTABLE | Permissions::PRINTABLE_IN_HIGH_QUALITY);
    }
    if !o.allow_copy {
        perms.remove(Permissions::COPYABLE);
    }
    if !o.allow_modify {
        perms.remove(Permissions::MODIFIABLE | Permissions::ANNOTABLE | Permissions::FILLABLE | Permissions::ASSEMBLABLE);
    }
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);
    let version = EncryptionVersion::V5 {
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), filter)]),
        file_encryption_key: &key,
        stream_filter: b"StdCF".to_vec(),
        string_filter: b"StdCF".to_vec(),
        owner_password: &owner,
        user_password: &o.user_password,
        permissions: perms,
    };
    let state = EncryptionState::try_from(version).map_err(err)?;
    // Objects inside object streams cannot be encrypted one by one.
    doc.decompress();
    doc.encrypt(&state).map_err(err)?;
    // Spell out the key length: some readers (Poppler, older Acrobat) expect it.
    if let Ok(id) = doc.trailer.get(b"Encrypt").and_then(|o| o.as_reference()) {
        if let Ok(d) = doc.get_object_mut(id).and_then(|o| o.as_dict_mut()) {
            d.set("Length", 256);
            if let Ok(lopdf::Object::Dictionary(cf)) = d.get_mut(b"CF") {
                if let Ok(lopdf::Object::Dictionary(std)) = cf.get_mut(b"StdCF") {
                    std.set("Length", 32);
                    std.set("AuthEvent", "DocOpen");
                }
            }
        }
    }
    let mut out = vec![];
    doc.save_to(&mut out).map_err(err)?;
    Ok(out)
}
