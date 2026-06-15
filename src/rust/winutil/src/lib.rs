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
use windows::Win32::System::SystemServices::{VER_EQUAL, VER_GREATER_EQUAL, VER_LESS};

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
}
