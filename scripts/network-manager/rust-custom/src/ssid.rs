//! Locale-guided SSID display conversion, separate from raw-byte identity.

use libc::{iconv, iconv_close, iconv_open};
pub(crate) fn ssid_convert(bytes: &[u8], encoding: &str) -> Option<String> {
    let encoding = std::ffi::CString::new(encoding).ok()?;
    let converter = unsafe { iconv_open(c"UTF-8".as_ptr(), encoding.as_ptr()) };
    if converter as usize == usize::MAX {
        return None;
    }
    let mut input = bytes.as_ptr().cast_mut().cast::<libc::c_char>();
    let mut input_left = bytes.len();
    let mut result = vec![0u8; bytes.len().checked_mul(6)?.checked_add(32)?];
    let mut output = result.as_mut_ptr().cast::<libc::c_char>();
    let mut output_left = result.len();
    let converted = unsafe {
        iconv(
            converter,
            &mut input,
            &mut input_left,
            &mut output,
            &mut output_left,
        )
    };
    unsafe {
        iconv_close(converter);
    }
    if converted == usize::MAX || input_left != 0 {
        return None;
    }
    result.truncate(result.len() - output_left);
    String::from_utf8(result)
        .ok()
        .map(|text| text.split('\0').next().unwrap_or("").to_owned())
}
pub(crate) fn ssid_encodings(locale: &str) -> Vec<String> {
    let language = locale.split('.').next().unwrap_or("").to_ascii_lowercase();
    let names: &[&str] = match language.as_str() {
        "zh_cn" | "zh_sg" => &["euc-cn", "gb2312", "gb18030"],
        "zh_hk" => &["big5", "euc-tw", "big5-hkcs"],
        "zh_mo" | "zh_tw" => &["big5", "euc-tw"],
        _ => match language.get(..2).unwrap_or("") {
            "ar" => &["iso-8859-6", "windows-1256"],
            "be" | "mk" | "ru" | "sr" => &["koi8-r", "windows-1251", "iso-8859-5"],
            "bg" => &["windows-1251", "koi8-r", "iso-8859-5"],
            "cs" | "hr" | "hu" | "pl" | "ro" | "sh" | "sk" | "sl" => {
                &["iso-8859-2", "windows-1250"]
            }
            "el" => &["iso-8859-7", "windows-1253"],
            "et" | "lt" | "lv" => &["iso-8859-4", "windows-1257"],
            "he" | "iw" => &["iso-8859-8", "windows-1255"],
            "ja" => &["euc-jp", "shift_jis", "iso-2022-jp"],
            "ko" => &["euc-kr", "iso-2022-kr", "johab"],
            "th" => &["iso-8859-11", "windows-874"],
            "tr" => &["iso-8859-9", "windows-1254"],
            "uk" => &["koi8-u", "koi8-r", "windows-1251"],
            _ => &[],
        },
    };
    if !names.is_empty() {
        return names.iter().map(|name| (*name).to_owned()).collect();
    }
    vec![
        locale
            .split_once('.')
            .map(|(_, encoding)| encoding.split('@').next().unwrap_or("UTF-8"))
            .unwrap_or("UTF-8")
            .to_owned(),
        "iso-8859-1".to_owned(),
        "windows-1251".to_owned(),
    ]
}
pub(crate) fn ssid_text(bytes: &[u8]) -> String {
    // Match NM's locale-guided display conversion, but never use its text as
    // identity: different SSID byte strings can have the same visible name.
    if !bytes.contains(&0)
        && let Ok(text) = std::str::from_utf8(bytes)
    {
        return text.to_owned();
    }
    static ENCODINGS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    let encodings = ENCODINGS.get_or_init(|| {
        let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .find_map(|name| std::env::var(name).ok())
            .unwrap_or_default();
        ssid_encodings(&locale)
    });
    encodings
        .iter()
        .find_map(|encoding| ssid_convert(bytes, encoding))
        .unwrap_or_else(|| {
            bytes
                .iter()
                .take_while(|byte| **byte != 0)
                .map(|byte| {
                    if (0x20..=0x7e).contains(byte) {
                        *byte as char
                    } else {
                        '?'
                    }
                })
                .collect()
        })
}
