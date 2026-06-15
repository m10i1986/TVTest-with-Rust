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

//! TVTest `PathUtil` のうち、プラットフォーム非依存なパス文字列操作を Rust に移植したもの。
//!
//! 原実装 (`src/PathUtil.cpp`) は Windows パス(UTF-16)を扱い、区切り文字 `\` `/`、
//! 拡張子区切り `.`、ドライブ区切り `:` のみを特別扱いする。
//! したがって本クレートも文字列を `&[u16]` / `Vec<u16>` として扱い、挙動を厳密一致させる。
//!
//! Win32 API 依存関数(`AppendDelimiter`/`IsRoot`/`IsExists`/`IsFileExists`)は
//! Rust 単体では等価検証できないため、本移植の対象外とする。

const BACKSLASH: u16 = b'\\' as u16;
const SLASH: u16 = b'/' as u16;
const DOT: u16 = b'.' as u16;
const COLON: u16 = b':' as u16;

/// 区切り文字判定。原実装 `IsDelimiter` (PathUtil.h:35)。
pub fn is_delimiter(c: u16) -> bool {
    c == BACKSLASH || c == SLASH
}

fn is_path_delimiter(c: &u16) -> bool {
    *c == BACKSLASH || *c == SLASH
}

/// 拡張子(最後の `.` 以降)を取り除く。原実装 `RemoveExtension` (PathUtil.cpp:33)。
///
/// 最後の `.` 以降に区切り文字が含まれない場合のみ拡張子とみなす
/// (ディレクトリ名中の `.` を誤認しない)。
pub fn remove_extension(path: &mut Vec<u16>) -> bool {
    if let Some(pos) = path.iter().rposition(|&c| c == DOT) {
        if !path[pos + 1..].iter().any(is_path_delimiter) {
            path.truncate(pos);
        }
    }
    true
}

/// 拡張子を付け替える。原実装 `RenameExtension` (PathUtil.cpp:46)。
///
/// `extension` は `.ext` のように `.` を含む文字列を想定(原実装どおり単純連結)。
pub fn rename_extension(path: &mut Vec<u16>, extension: &[u16]) -> bool {
    remove_extension(path);
    if !extension.is_empty() {
        path.extend_from_slice(extension);
    }
    true
}

/// 拡張子(`.` を含む)を取り出す。無ければ空。原実装 `GetExtension` (PathUtil.cpp:59)。
pub fn get_extension(path: &[u16]) -> Vec<u16> {
    if let Some(pos) = path.iter().rposition(|&c| c == DOT) {
        if !path[pos + 1..].iter().any(is_path_delimiter) {
            return path[pos..].to_vec();
        }
    }
    Vec::new()
}

/// ファイル名部分を取り除きディレクトリ部にする。原実装 `RemoveFileName` (PathUtil.cpp:74)。
///
/// 変更があれば true。区切りが無い、または既にディレクトリ末尾の場合は false。
pub fn remove_file_name(path: &mut Vec<u16>) -> bool {
    let pos = match path.iter().rposition(is_path_delimiter) {
        Some(p) => p,
        None => return false,
    };
    // "C:\" のように区切り直前が ':' のときは区切りを残す。
    let pos = if pos > 0 && path[pos - 1] == COLON {
        pos + 1
    } else {
        pos
    };
    if pos == path.len() {
        return false;
    }
    path.truncate(pos);
    true
}

/// パスに要素を追加する。原実装 `Append`(LPCWSTR 版) (PathUtil.cpp:95)。
///
/// 末尾に区切りが無ければ `\` を補い、`more` 先頭の区切りは取り除いてから連結する。
pub fn append(path: &mut Vec<u16>, more: &[u16]) -> bool {
    if !path.is_empty() && !is_delimiter(*path.last().unwrap()) {
        path.push(BACKSLASH);
    }
    if !more.is_empty() {
        if is_delimiter(more[0]) {
            path.extend_from_slice(&more[1..]);
        } else {
            path.extend_from_slice(more);
        }
    }
    true
}

/// ファイル名部分(最後の区切り以降)を取り出す。原実装 `GetFileName` (PathUtil.cpp:128)。
pub fn get_file_name(path: &[u16]) -> Vec<u16> {
    match path.iter().rposition(is_path_delimiter) {
        Some(pos) => path[pos + 1..].to_vec(),
        None => path.to_vec(),
    }
}

/// パスをディレクトリ部とファイル名部に分割する。原実装 `Split` (PathUtil.cpp:143)。
///
/// 戻り値は `(directory, file_name)`。
pub fn split(path: &[u16]) -> (Vec<u16>, Vec<u16>) {
    match path.iter().rposition(is_path_delimiter) {
        None => (Vec::new(), path.to_vec()),
        Some(mut pos) => {
            let file_name = path[pos + 1..].to_vec();
            // "C:\file" のように区切り直前が ':' のときは区切りをディレクトリ側に含める。
            if pos > 0 && path[pos - 1] == COLON {
                pos += 1;
            }
            let directory = path[..pos].to_vec();
            (directory, file_name)
        }
    }
}

/// 末尾の区切り文字を全て取り除く。原実装 `RemoveDelimiter` (PathUtil.cpp:183)。
///
/// 空文字列のときは何もせず false。
pub fn remove_delimiter(path: &mut Vec<u16>) -> bool {
    if path.is_empty() {
        return false;
    }
    let mut pos = path.len();
    while pos > 0 && is_delimiter(path[pos - 1]) {
        pos -= 1;
    }
    path.truncate(pos);
    true
}

/// 絶対パスか判定。原実装 `IsAbsolute` (PathUtil.cpp:198)。
///
/// UNC(`\\`)またはドライブ指定(`X:`)を絶対パスとみなす。
pub fn is_absolute(path: &[u16]) -> bool {
    path.len() >= 2 && ((is_delimiter(path[0]) && is_delimiter(path[1])) || path[1] == COLON)
}

/// 絶対パスでないか判定。原実装 `IsRelative` (PathUtil.h:47)。
pub fn is_relative(path: &[u16]) -> bool {
    !is_absolute(path)
}

/// 相対パスを基準パスから絶対パスへ変換。原実装 `RelativeToAbsolute` (PathUtil.cpp:206)。
///
/// 成功時 `Some(絶対パス)`。基準が空で相対も絶対でない、または正規化失敗で `None`。
pub fn relative_to_absolute(base_path: &[u16], relative_path: &[u16]) -> Option<Vec<u16>> {
    if base_path.is_empty() {
        if is_absolute(relative_path) {
            return Some(relative_path.to_vec());
        }
        return None;
    }
    if relative_path.is_empty() {
        return Some(base_path.to_vec());
    }
    let mut path = base_path.to_vec();
    append(&mut path, relative_path);
    if !canonicalize(&mut path) {
        return None;
    }
    Some(path)
}

/// パスを正規化(`.` / `..` を解決)する。原実装 `Canonicalize` (PathUtil.cpp:237)。
///
/// 原実装はバックスラッシュ区切りのみを対象とする(スラッシュは要素文字扱い)。
/// `..` が先頭を遡ろうとする場合は false。
pub fn canonicalize(path: &mut Vec<u16>) -> bool {
    let mut next: usize = 0;

    loop {
        // 現在位置以降の最初の '\\' を探す。無ければ末尾。
        let pos = path[next..]
            .iter()
            .position(|&c| c == BACKSLASH)
            .map(|p| p + next)
            .unwrap_or(path.len());

        if pos > next {
            let item = &path[next..pos];
            if item == [DOT] {
                // "." とその直後の区切りを削除。
                let erase_len = (pos - next + 1).min(path.len() - next);
                path.drain(next..next + erase_len);
                // next は据え置き(同じ位置から再評価)。
            } else if item == [DOT, DOT] {
                if next < 2 {
                    return false;
                }
                // 直前の区切りを探す(next-2 以前)。
                let prev = match path[..next - 1].iter().rposition(|&c| c == BACKSLASH) {
                    Some(p) => p,
                    None => return false,
                };
                path.drain(prev..pos);
                next = prev + 1;
            } else {
                next = pos + 1;
            }
        } else {
            next = pos + 1;
        }

        if next >= path.len() {
            break;
        }
    }

    // "X:" のみなら末尾に '\' を補う。
    if path.len() == 2 && path[1] == COLON {
        path.push(BACKSLASH);
    }

    true
}

// ---------------------------------------------------------------------------
// テスト用ヘルパ
// ---------------------------------------------------------------------------

/// `&str` を UTF-16 コードユニット列に変換するヘルパ。
pub fn to_u16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// UTF-16 コードユニット列を `String` に変換するヘルパ。
pub fn from_u16(s: &[u16]) -> String {
    String::from_utf16_lossy(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Vec<u16> {
        to_u16(s)
    }

    #[test]
    fn test_is_delimiter() {
        assert!(is_delimiter(BACKSLASH));
        assert!(is_delimiter(SLASH));
        assert!(!is_delimiter(DOT));
    }

    #[test]
    fn test_remove_extension() {
        let mut p = u("file.txt");
        remove_extension(&mut p);
        assert_eq!(from_u16(&p), "file");

        let mut p = u("C:\\dir.name\\file");
        remove_extension(&mut p);
        // 最後の '.' 以降に区切りがあるので拡張子とみなさない
        assert_eq!(from_u16(&p), "C:\\dir.name\\file");

        let mut p = u("archive.tar.gz");
        remove_extension(&mut p);
        assert_eq!(from_u16(&p), "archive.tar");

        let mut p = u("noext");
        remove_extension(&mut p);
        assert_eq!(from_u16(&p), "noext");
    }

    #[test]
    fn test_rename_extension() {
        let mut p = u("file.txt");
        rename_extension(&mut p, &u(".dat"));
        assert_eq!(from_u16(&p), "file.dat");

        let mut p = u("file.txt");
        rename_extension(&mut p, &u("")); // 拡張子削除
        assert_eq!(from_u16(&p), "file");
    }

    #[test]
    fn test_get_extension() {
        assert_eq!(from_u16(&get_extension(&u("file.txt"))), ".txt");
        assert_eq!(from_u16(&get_extension(&u("noext"))), "");
        assert_eq!(from_u16(&get_extension(&u("C:\\dir.x\\file"))), "");
        assert_eq!(from_u16(&get_extension(&u("a.b.c"))), ".c");
    }

    #[test]
    fn test_remove_file_name() {
        let mut p = u("C:\\dir\\file.txt");
        assert!(remove_file_name(&mut p));
        assert_eq!(from_u16(&p), "C:\\dir");

        // ドライブ直下はバックスラッシュを残す
        let mut p = u("C:\\file.txt");
        assert!(remove_file_name(&mut p));
        assert_eq!(from_u16(&p), "C:\\");

        // 区切りが無い
        let mut p = u("file.txt");
        assert!(!remove_file_name(&mut p));
        assert_eq!(from_u16(&p), "file.txt");
    }

    #[test]
    fn test_append() {
        let mut p = u("C:\\dir");
        append(&mut p, &u("file.txt"));
        assert_eq!(from_u16(&p), "C:\\dir\\file.txt");

        // 末尾に区切りがあれば二重にしない
        let mut p = u("C:\\dir\\");
        append(&mut p, &u("file.txt"));
        assert_eq!(from_u16(&p), "C:\\dir\\file.txt");

        // more 先頭の区切りは除去
        let mut p = u("C:\\dir");
        append(&mut p, &u("\\file.txt"));
        assert_eq!(from_u16(&p), "C:\\dir\\file.txt");

        // 空パスへの追加
        let mut p = u("");
        append(&mut p, &u("file.txt"));
        assert_eq!(from_u16(&p), "file.txt");
    }

    #[test]
    fn test_get_file_name() {
        assert_eq!(from_u16(&get_file_name(&u("C:\\dir\\file.txt"))), "file.txt");
        assert_eq!(from_u16(&get_file_name(&u("file.txt"))), "file.txt");
        assert_eq!(from_u16(&get_file_name(&u("dir/file"))), "file");
    }

    #[test]
    fn test_split() {
        let (d, f) = split(&u("C:\\dir\\file.txt"));
        assert_eq!(from_u16(&d), "C:\\dir");
        assert_eq!(from_u16(&f), "file.txt");

        // ドライブ直下
        let (d, f) = split(&u("C:\\file.txt"));
        assert_eq!(from_u16(&d), "C:\\");
        assert_eq!(from_u16(&f), "file.txt");

        // 区切り無し
        let (d, f) = split(&u("file.txt"));
        assert_eq!(from_u16(&d), "");
        assert_eq!(from_u16(&f), "file.txt");
    }

    #[test]
    fn test_remove_delimiter() {
        let mut p = u("C:\\dir\\\\");
        assert!(remove_delimiter(&mut p));
        assert_eq!(from_u16(&p), "C:\\dir");

        let mut p = u("C:\\dir");
        assert!(remove_delimiter(&mut p));
        assert_eq!(from_u16(&p), "C:\\dir");

        let mut p = u("");
        assert!(!remove_delimiter(&mut p));
    }

    #[test]
    fn test_is_absolute() {
        assert!(is_absolute(&u("C:\\dir")));
        assert!(is_absolute(&u("\\\\server\\share")));
        assert!(!is_absolute(&u("dir\\file")));
        assert!(!is_absolute(&u("a")));
        assert!(is_relative(&u("dir\\file")));
    }

    #[test]
    fn test_canonicalize() {
        let mut p = u("C:\\dir\\.\\file");
        assert!(canonicalize(&mut p));
        assert_eq!(from_u16(&p), "C:\\dir\\file");

        let mut p = u("C:\\dir\\sub\\..\\file");
        assert!(canonicalize(&mut p));
        assert_eq!(from_u16(&p), "C:\\dir\\file");

        // ドライブのみは末尾に '\' を補う
        let mut p = u("C:");
        assert!(canonicalize(&mut p));
        assert_eq!(from_u16(&p), "C:\\");
    }

    #[test]
    fn test_relative_to_absolute() {
        let r = relative_to_absolute(&u("C:\\base"), &u("sub\\file.txt"));
        assert_eq!(from_u16(&r.unwrap()), "C:\\base\\sub\\file.txt");

        // 親参照
        let r = relative_to_absolute(&u("C:\\base\\dir"), &u("..\\file.txt"));
        assert_eq!(from_u16(&r.unwrap()), "C:\\base\\file.txt");

        // 基準空 + 相対が絶対
        let r = relative_to_absolute(&u(""), &u("C:\\abs"));
        assert_eq!(from_u16(&r.unwrap()), "C:\\abs");

        // 基準空 + 相対も相対 → None
        assert!(relative_to_absolute(&u(""), &u("rel")).is_none());

        // 相対が空 → 基準そのもの
        let r = relative_to_absolute(&u("C:\\base"), &u(""));
        assert_eq!(from_u16(&r.unwrap()), "C:\\base");
    }
}
