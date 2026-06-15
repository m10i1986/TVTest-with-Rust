/*
  TVTest
  Copyright(c) 2008-2020 DBCTRADO

  This program is free software; you can redistribute it and/or modify
  it under the terms of the GNU General Public License as published by
  the Free Software Foundation; either version 2 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU General Public License for more details.

  You should have received a copy of the GNU General Public License
  along with this program; if not, write to the Free Software
  Foundation, Inc., 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA
*/

//! TVTest `Util` のうち Win32 API に依存する関数を、`windows` クレート(windows-rs)で
//! Rust に移植したもの。原実装 (`src/Util.cpp`)。
//!
//! 純粋計算のみの関数は [`tvtest_util`] 相当のクレートにあり、本クレートは実際に
//! Windows API を呼ぶ部分(OS バージョン判定・エラーテキスト取得など)を担当する。
//!
//! windows-rs を用いた移植が成立することを実証する足場でもある。

#![cfg(windows)]

use windows::Win32::System::Diagnostics::Debug::{
    FormatMessageW, FORMAT_MESSAGE_FROM_SYSTEM, FORMAT_MESSAGE_IGNORE_INSERTS,
};
use windows::Win32::System::SystemInformation::{
    VerSetConditionMask, VerifyVersionInfoW, OSVERSIONINFOEXW, VER_BUILDNUMBER, VER_MAJORVERSION,
    VER_MINORVERSION,
};
use windows::Win32::Globalization::{CompareStringOrdinal, CSTR_EQUAL};
use windows::Win32::Storage::FileSystem::{GetFileAttributesW, INVALID_FILE_ATTRIBUTES};
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::SystemServices::{VER_EQUAL, VER_GREATER_EQUAL, VER_LESS};

use tvtest_path_util as path_util;

/// Windows の `MAX_PATH`。
pub const MAX_PATH: usize = 260;

// 原実装の `OS` 名前空間に相当。

/// メジャー/マイナー(オプションでビルド)バージョンを指定演算子で比較する。
/// 原実装 `OS::VerifyOSVersion` (Util.cpp:1566, 1582)。
///
/// `*_op` には `VER_EQUAL` / `VER_GREATER_EQUAL` / `VER_LESS` 等を渡す。
/// `build` が `Some` のときのみビルド番号も条件に含める。
fn verify_os_version(
    major: u32,
    major_op: u8,
    minor: u32,
    minor_op: u8,
    build: Option<(u32, u8)>,
) -> bool {
    let mut osvi = OSVERSIONINFOEXW {
        dwOSVersionInfoSize: std::mem::size_of::<OSVERSIONINFOEXW>() as u32,
        dwMajorVersion: major,
        dwMinorVersion: minor,
        ..Default::default()
    };

    // 比較条件マスクを組み立てる(VerSetConditionMask は新しいマスクを返す)。
    let mut type_mask = VER_MAJORVERSION | VER_MINORVERSION;
    let mut condition = unsafe {
        VerSetConditionMask(
            VerSetConditionMask(0, VER_MAJORVERSION, major_op),
            VER_MINORVERSION,
            minor_op,
        )
    };

    if let Some((build_no, build_op)) = build {
        osvi.dwBuildNumber = build_no;
        type_mask |= VER_BUILDNUMBER;
        condition = unsafe { VerSetConditionMask(condition, VER_BUILDNUMBER, build_op) };
    }

    unsafe { VerifyVersionInfoW(&mut osvi, type_mask, condition).is_ok() }
}

fn check_os_version(major: u32, minor: u32) -> bool {
    verify_os_version(major, VER_EQUAL as u8, minor, VER_EQUAL as u8, None)
}

// 原実装 `OS::CheckOSVersion`(ビルド指定・完全一致)に対応。現状は呼び出し元が無いが
// 原実装との対応を保つため残す。
#[allow(dead_code)]
fn check_os_version_build(major: u32, minor: u32, build: u32) -> bool {
    verify_os_version(
        major,
        VER_EQUAL as u8,
        minor,
        VER_EQUAL as u8,
        Some((build, VER_EQUAL as u8)),
    )
}

fn check_os_version_later(major: u32, minor: u32) -> bool {
    verify_os_version(
        major,
        VER_GREATER_EQUAL as u8,
        minor,
        VER_GREATER_EQUAL as u8,
        None,
    )
}

fn check_os_version_later_build(major: u32, minor: u32, build: u32) -> bool {
    verify_os_version(
        major,
        VER_GREATER_EQUAL as u8,
        minor,
        VER_GREATER_EQUAL as u8,
        Some((build, VER_GREATER_EQUAL as u8)),
    )
}

/// 原実装 `OS::IsWindowsXP` (Util.cpp:1624)。
pub fn is_windows_xp() -> bool {
    verify_os_version(5, VER_EQUAL as u8, 1, VER_GREATER_EQUAL as u8, None)
}

/// 原実装 `OS::IsWindowsVista` (Util.cpp:1629)。
pub fn is_windows_vista() -> bool {
    check_os_version(6, 0)
}

/// 原実装 `OS::IsWindows7` (Util.cpp:1634)。
pub fn is_windows_7() -> bool {
    check_os_version(6, 1)
}

/// 原実装 `OS::IsWindows8` (Util.cpp:1639)。
pub fn is_windows_8() -> bool {
    check_os_version(6, 2)
}

/// 原実装 `OS::IsWindows8_1` (Util.cpp:1644)。
pub fn is_windows_8_1() -> bool {
    check_os_version(6, 3)
}

/// 原実装 `OS::IsWindows10` (Util.cpp:1649)。ビルド 22000 未満を Windows 10 とみなす。
pub fn is_windows_10() -> bool {
    verify_os_version(
        10,
        VER_EQUAL as u8,
        0,
        VER_EQUAL as u8,
        Some((22000, VER_LESS as u8)),
    )
}

/// 原実装 `OS::IsWindowsXPOrLater` (Util.cpp:1654)。
pub fn is_windows_xp_or_later() -> bool {
    check_os_version_later(5, 1)
}

/// 原実装 `OS::IsWindowsVistaOrLater` (Util.cpp:1659)。
pub fn is_windows_vista_or_later() -> bool {
    check_os_version_later(6, 0)
}

/// 原実装 `OS::IsWindows7OrLater` (Util.cpp:1664)。
pub fn is_windows_7_or_later() -> bool {
    check_os_version_later(6, 1)
}

/// 原実装 `OS::IsWindows8OrLater` (Util.cpp:1669)。
pub fn is_windows_8_or_later() -> bool {
    check_os_version_later(6, 2)
}

/// 原実装 `OS::IsWindows8_1OrLater` (Util.cpp:1674)。
pub fn is_windows_8_1_or_later() -> bool {
    check_os_version_later(6, 3)
}

/// 原実装 `OS::IsWindows10OrLater` (Util.cpp:1679)。
pub fn is_windows_10_or_later() -> bool {
    check_os_version_later(10, 0)
}

/// 原実装 `OS::IsWindows10AnniversaryUpdateOrLater` (Util.cpp:1684)。
pub fn is_windows_10_anniversary_update_or_later() -> bool {
    check_os_version_later_build(10, 0, 14393)
}

/// 原実装 `OS::IsWindows10CreatorsUpdateOrLater` (Util.cpp:1689)。
pub fn is_windows_10_creators_update_or_later() -> bool {
    check_os_version_later_build(10, 0, 15063)
}

/// 原実装 `OS::IsWindows10RS5OrLater` (Util.cpp:1694)。
pub fn is_windows_10_rs5_or_later() -> bool {
    check_os_version_later_build(10, 0, 17763)
}

/// 原実装 `OS::IsWindows10_19H1OrLater` (Util.cpp:1699)。
pub fn is_windows_10_19h1_or_later() -> bool {
    check_os_version_later_build(10, 0, 18362)
}

/// 原実装 `OS::IsWindows10_20H1OrLater` (Util.cpp:1704)。
pub fn is_windows_10_20h1_or_later() -> bool {
    check_os_version_later_build(10, 0, 19041)
}

/// 原実装 `OS::IsWindows11` (Util.cpp:1709)。ビルド 22000 以上。
pub fn is_windows_11() -> bool {
    check_os_version_later_build(10, 0, 22000)
}

/// 原実装 `OS::IsWindows11OrLater` (Util.cpp:1715)。
pub fn is_windows_11_or_later() -> bool {
    check_os_version_later_build(10, 0, 22000)
}

/// エラーコードに対応するシステムメッセージ文字列を取得する。
/// 原実装 `GetErrorText` (Util.cpp:650)。
///
/// 原実装は呼び出し側バッファに書き込むが、Rust では `String` を返す形にする。
/// 取得できない場合は空文字列。
pub fn get_error_text(error_code: u32) -> String {
    const LANG_NEUTRAL: u32 = 0x00;
    const SUBLANG_DEFAULT: u32 = 0x01;
    // MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT)
    let lang_id = (SUBLANG_DEFAULT << 10) | LANG_NEUTRAL;

    let mut buffer = [0u16; 1024];
    let length = unsafe {
        FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
            None,
            error_code,
            lang_id,
            windows::core::PWSTR(buffer.as_mut_ptr()),
            buffer.len() as u32,
            None,
        )
    };

    if length == 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buffer[..length as usize])
}

// ---------------------------------------------------------------------------
// ファイル名
// ---------------------------------------------------------------------------

/// 2つのファイル名を大文字小文字を無視して比較し、等しいか判定する。
/// 原実装 `IsEqualFileName` (Util.cpp:665)。`CompareStringOrdinal` を使用。
pub fn is_equal_file_name(name1: &[u16], name2: &[u16]) -> bool {
    // -1 ではなくスライス長を渡す(NUL 終端に依存しない)。
    unsafe { CompareStringOrdinal(name1, name2, true) == CSTR_EQUAL }
}

/// [`is_valid_file_name`] のフラグ。原実装 `FileNameValidateFlag` (Util.h:105)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FileNameValidateFlag {
    /// ワイルドカード `*` `?` を許可する。
    pub wildcard: bool,
    /// パス区切り `\` を許可する。
    pub allow_delimiter: bool,
}

/// ファイル名検証の結果。`Ok(())` なら有効、`Err(メッセージ)` なら無効。
pub type ValidateResult = Result<(), String>;

/// 予約デバイス名(CON/PRN/AUX/NUL/COM1-9/LPT1-9)に大小無視で一致するか。
fn is_reserved_device_name(name: &[u16]) -> bool {
    fn eq_ascii_ci(name: &[u16], target: &str) -> bool {
        let t: Vec<u16> = target.encode_utf16().collect();
        if name.len() != t.len() {
            return false;
        }
        name.iter().zip(t.iter()).all(|(&a, &b)| {
            let la = if (b'A' as u16..=b'Z' as u16).contains(&a) { a + 32 } else { a };
            let lb = if (b'A' as u16..=b'Z' as u16).contains(&b) { b + 32 } else { b };
            la == lb
        })
    }

    match name.len() {
        3 => ["CON", "PRN", "AUX", "NUL"]
            .iter()
            .any(|d| eq_ascii_ci(name, d)),
        4 => (1..=9).any(|i| {
            eq_ascii_ci(name, &format!("COM{}", i)) || eq_ascii_ci(name, &format!("LPT{}", i))
        }),
        _ => false,
    }
}

/// ファイル名として有効か検証する。原実装 `IsValidFileName` (Util.cpp:671)。
///
/// 予約デバイス名・禁止文字・長さ・末尾の空白/ドットを検査する。
/// 文字検証は ASCII の禁止文字に基づくため Win32 非依存だが、本クレートに置いて
/// `is_equal_file_name` などのファイル名系 API と集約する。
pub fn is_valid_file_name(name: &[u16], flags: FileNameValidateFlag) -> ValidateResult {
    if name.is_empty() {
        return Err("ファイル名が指定されていません。".to_string());
    }
    if name.len() >= MAX_PATH {
        return Err("ファイル名が長すぎます。".to_string());
    }
    if is_reserved_device_name(name) {
        return Err("仮想デバイス名はファイル名に使用できません。".to_string());
    }

    for (i, &c) in name.iter().enumerate() {
        let is_forbidden = c <= 31
            || c == b'<' as u16
            || c == b'>' as u16
            || c == b':' as u16
            || c == b'"' as u16
            || c == b'/' as u16
            || c == b'|' as u16
            || (!flags.wildcard && (c == b'*' as u16 || c == b'?' as u16))
            || (!flags.allow_delimiter && c == b'\\' as u16);
        if is_forbidden {
            let msg = if c <= 31 {
                // 原実装の "{:#02x}" 相当(0x プレフィックス付き)。
                format!("ファイル名に使用できない文字 {:#02x} が含まれています。", c)
            } else {
                let ch = char::from_u32(c as u32).unwrap_or('?');
                format!("ファイル名に使用できない文字 {} が含まれています。", ch)
            };
            return Err(msg);
        }
        // 末尾の半角空白・ドットは不可。
        let is_last = i + 1 == name.len();
        if is_last && (c == b' ' as u16 || c == b'.' as u16) {
            return Err("ファイル名の末尾に半角空白及び . は使用できません。".to_string());
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// パス(実行ファイル基準・存在確認)
// ---------------------------------------------------------------------------

/// パスが存在するか(ファイル/ディレクトリを問わず)。Win32 `PathFileExists` 相当。
/// `GetFileAttributesW` が `INVALID_FILE_ATTRIBUTES` 以外を返すかで判定する。
pub fn path_exists(path: &[u16]) -> bool {
    let mut buf: Vec<u16> = path.to_vec();
    buf.push(0); // NUL 終端
    let attrs = unsafe { GetFileAttributesW(windows::core::PCWSTR(buf.as_ptr())) };
    attrs != INVALID_FILE_ATTRIBUTES
}

/// 自プロセスの実行ファイルのフルパスを取得する。Win32 `GetModuleFileNameW(nullptr, ...)` 相当。
pub fn get_module_file_name() -> Vec<u16> {
    let mut buf = [0u16; MAX_PATH];
    let len = unsafe { GetModuleFileNameW(None, &mut buf) };
    buf[..len as usize].to_vec()
}

/// 相対パスを実行ファイルのあるディレクトリ基準で絶対パス化する。
/// 原実装 `GetAbsolutePath`(String 版) (Util.cpp:843)。
///
/// 既に絶対パスならそのまま返す。空入力や正規化失敗で `None`。
pub fn get_absolute_path(file_path: &[u16]) -> Option<Vec<u16>> {
    if file_path.is_empty() {
        return None;
    }
    if path_util::is_relative(file_path) {
        // 実行ファイルパスからファイル名を除いたディレクトリを基準にする。
        let mut dir = get_module_file_name();
        // 原実装 PathRemoveFileSpec 相当(末尾のファイル名を除去)。
        path_util::remove_file_name(&mut dir);
        path_util::relative_to_absolute(&dir, file_path)
    } else {
        Some(file_path.to_vec())
    }
}

/// ファイル名が既存と衝突する場合、連番を付けて一意なパスを作る。
/// 原実装 `MakeUniqueFileName` (Util.cpp:760)。
///
/// `max_length` は生成パスの最大長、`number_format` は連番書式(`None` なら `-{n}`)。
/// 連番は 2 から始め、`MAX_NUMBER`(1000)未満まで試す。成功時に新パスを返す。
///
/// `number_format` は `{}` を連番に置換するシンプルな書式とする(原実装の `StringVFormat` 相当範囲)。
pub fn make_unique_file_name(
    file_name: &[u16],
    max_length: usize,
    number_format: Option<&str>,
) -> Option<Vec<u16>> {
    if file_name.is_empty() {
        return None;
    }

    // ディレクトリ部の長さ(末尾区切りまで含む)。PathFindFileName 相当。
    let dir_length = dir_prefix_len(file_name);
    if dir_length == 0 || dir_length > max_length.saturating_sub(1) {
        return None;
    }

    let extension = path_util::get_extension(file_name);
    let extension_length = extension.len();
    let max_file_name = max_length - dir_length;

    // まず長さ制限に収める。
    let mut path: Vec<u16> = file_name.to_vec();
    if path.len() > max_length {
        if extension_length < max_file_name {
            path.truncate(max_length - extension_length);
            path.extend_from_slice(&extension);
        } else {
            path.truncate(max_length);
        }
    }

    if path_exists(&path) {
        const MAX_NUMBER: i32 = 1000;
        // 拡張子を除いたファイル名本体(ディレクトリ部以降)。
        let base_name: Vec<u16> =
            file_name[dir_length..file_name.len() - extension_length].to_vec();
        let fmt = number_format.unwrap_or("-{}");

        let mut i = 2;
        loop {
            if i == MAX_NUMBER {
                return None;
            }
            let number: Vec<u16> = fmt.replace("{}", &i.to_string()).encode_utf16().collect();

            let mut name = base_name.clone();
            name.extend_from_slice(&number);
            name.extend_from_slice(&extension);

            let mut candidate: Vec<u16> = file_name[..dir_length].to_vec();
            if name.len() <= max_file_name {
                candidate.extend_from_slice(&name);
            } else {
                // 長すぎる場合は末尾 max_file_name 文字を採用。
                candidate.extend_from_slice(&name[name.len() - max_file_name..]);
            }

            if !path_exists(&candidate) {
                path = candidate;
                break;
            }
            i += 1;
        }
    }

    Some(path)
}

/// パスからファイル名を除いた接頭辞(ディレクトリ部 + 区切り)の長さを返す。
/// Win32 `PathFindFileName(p) - p` 相当(最後の区切りの次の位置)。
fn dir_prefix_len(path: &[u16]) -> usize {
    match path
        .iter()
        .rposition(|&c| c == b'\\' as u16 || c == b'/' as u16)
    {
        Some(pos) => pos + 1,
        None => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_os_version_ordering_is_consistent() {
        // 実行中の OS で必ず成り立つ単調性を検証する。
        // 「Vista 以降」が真なら「XP 以降」も真、のように後方の条件は前方を含意する。
        if is_windows_vista_or_later() {
            assert!(is_windows_xp_or_later());
        }
        if is_windows_7_or_later() {
            assert!(is_windows_vista_or_later());
        }
        if is_windows_10_or_later() {
            assert!(is_windows_8_1_or_later());
        }
        if is_windows_11_or_later() {
            assert!(is_windows_10_or_later());
        }
    }

    #[test]
    fn test_running_on_some_known_windows() {
        // CI/開発機は Windows 7 以降のはずなので、少なくとも XP 以降は真。
        assert!(is_windows_xp_or_later());
    }

    #[test]
    fn test_win10_and_win11_are_mutually_exclusive() {
        // IsWindows10(22000未満)と IsWindows11(22000以上)は同時に真にならない。
        assert!(!(is_windows_10() && is_windows_11()));
    }

    #[test]
    fn test_get_error_text_known_code() {
        // ERROR_FILE_NOT_FOUND (2) は必ずメッセージを持つ。
        let text = get_error_text(2);
        assert!(!text.is_empty());
    }

    #[test]
    fn test_get_error_text_invalid_code() {
        // 存在しないであろう巨大なコードは空文字列になることが多い。
        // (環境によりメッセージを返す可能性もあるためパニックしないことのみ確認)
        let _ = get_error_text(0xFFFF_FFFE);
    }

    fn u(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    #[test]
    fn test_is_equal_file_name() {
        assert!(is_equal_file_name(&u("File.txt"), &u("file.TXT")));
        assert!(is_equal_file_name(&u("abc"), &u("abc")));
        assert!(!is_equal_file_name(&u("abc"), &u("abd")));
    }

    #[test]
    fn test_is_valid_file_name_ok() {
        let f = FileNameValidateFlag::default();
        assert!(is_valid_file_name(&u("normal_file.txt"), f).is_ok());
        assert!(is_valid_file_name(&u("日本語ファイル.mp4"), f).is_ok());
    }

    #[test]
    fn test_is_valid_file_name_empty() {
        let f = FileNameValidateFlag::default();
        assert!(is_valid_file_name(&u(""), f).is_err());
    }

    #[test]
    fn test_is_valid_file_name_reserved() {
        let f = FileNameValidateFlag::default();
        assert!(is_valid_file_name(&u("CON"), f).is_err());
        assert!(is_valid_file_name(&u("con"), f).is_err()); // 大小無視
        assert!(is_valid_file_name(&u("COM1"), f).is_err());
        assert!(is_valid_file_name(&u("LPT9"), f).is_err());
        // 予約名に似ているが別物は OK
        assert!(is_valid_file_name(&u("CONS"), f).is_ok());
        assert!(is_valid_file_name(&u("COM0"), f).is_ok());
    }

    #[test]
    fn test_is_valid_file_name_forbidden_chars() {
        let f = FileNameValidateFlag::default();
        assert!(is_valid_file_name(&u("a<b"), f).is_err());
        assert!(is_valid_file_name(&u("a:b"), f).is_err());
        assert!(is_valid_file_name(&u("a/b"), f).is_err());
        assert!(is_valid_file_name(&u("a|b"), f).is_err());
        // 既定ではワイルドカード・区切りも不可
        assert!(is_valid_file_name(&u("a*b"), f).is_err());
        assert!(is_valid_file_name(&u("a\\b"), f).is_err());
    }

    #[test]
    fn test_is_valid_file_name_flags() {
        // ワイルドカード許可
        let f = FileNameValidateFlag { wildcard: true, allow_delimiter: false };
        assert!(is_valid_file_name(&u("a*b?c"), f).is_ok());
        // 区切り許可
        let f = FileNameValidateFlag { wildcard: false, allow_delimiter: true };
        assert!(is_valid_file_name(&u("dir\\file"), f).is_ok());
    }

    #[test]
    fn test_is_valid_file_name_trailing() {
        let f = FileNameValidateFlag::default();
        assert!(is_valid_file_name(&u("file "), f).is_err()); // 末尾空白
        assert!(is_valid_file_name(&u("file."), f).is_err()); // 末尾ドット
        // 途中のドットは OK
        assert!(is_valid_file_name(&u("a.b.c"), f).is_ok());
    }

    #[test]
    fn test_path_exists() {
        // 実行ファイル自身は存在するはず。
        let exe = get_module_file_name();
        assert!(!exe.is_empty());
        assert!(path_exists(&exe));

        // 存在しないであろうパス
        assert!(!path_exists(&u("Z:\\no_such_dir\\no_such_file_xyz.tmp")));
    }

    #[test]
    fn test_get_module_file_name_is_absolute() {
        let exe = get_module_file_name();
        assert!(path_util::is_absolute(&exe));
    }

    #[test]
    fn test_get_absolute_path() {
        // 既に絶対パスならそのまま。
        let abs = u("C:\\dir\\file.txt");
        assert_eq!(get_absolute_path(&abs).unwrap(), abs);

        // 相対パスは実行ファイルのディレクトリ基準で絶対化される。
        let r = get_absolute_path(&u("config.ini")).unwrap();
        assert!(path_util::is_absolute(&r));
        assert!(from(&r).ends_with("config.ini"));

        // 空は None。
        assert!(get_absolute_path(&u("")).is_none());
    }

    #[test]
    fn test_make_unique_file_name_no_conflict() {
        // 存在しないパスはそのまま返る。
        let p = u("Z:\\no_such_dir\\unique_name.txt");
        let r = make_unique_file_name(&p, MAX_PATH - 1, None).unwrap();
        assert_eq!(r, p);
    }

    #[test]
    fn test_make_unique_file_name_with_conflict() {
        // 一時ディレクトリに実ファイルを作って衝突を再現する。
        let mut dir = std::env::temp_dir();
        dir.push(format!("tvtest_winutil_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let base = dir.join("sample.txt");
        std::fs::write(&base, b"x").unwrap();

        let base_u: Vec<u16> = base.to_string_lossy().encode_utf16().collect();
        let unique = make_unique_file_name(&base_u, MAX_PATH - 1, None).unwrap();

        // 衝突したので別名(sample-2.txt)になっているはず。
        let unique_s = from(&unique);
        assert_ne!(unique, base_u);
        assert!(unique_s.contains("sample-2.txt"), "got: {}", unique_s);
        assert!(!path_exists(&unique));

        // 後始末。
        std::fs::remove_dir_all(&dir).ok();
    }

    fn from(s: &[u16]) -> String {
        String::from_utf16_lossy(s)
    }
}
