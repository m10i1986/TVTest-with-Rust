//! TVTest の COM ユーティリティ(`src/ComUtility.cpp` / `src/ComUtility.h`)を
//! windows-rs で移植したクレート。
//!
//! 移植対象:
//! - [`CVariant`](`ComUtility.h:59-77`、`ComUtility.cpp:36-159`)。`VARIANT` の
//!   RAII ラッパー。構築時に `VariantInit`、破棄時に `VariantClear`、コピー時に
//!   `VariantCopy`(deep copy)を呼ぶ。`ChangeType`/`ToString`/`FromString` は
//!   `VariantChangeType` を `VARIANT_ALPHABOOL` フラグ付きで呼ぶ
//!   (`VT_BOOL` が "True"/"False" の文字列へ変換される)。
//! - [`CPropertyBag`](`ComUtility.h:79-103`、`ComUtility.cpp:164-219` の
//!   `Read`/`Write`)。
//!   キー文字列 → `CVariant` のプロパティ管理。原実装の `std::map<String, CVariant>`
//!   (大文字小文字を区別)に対応する `BTreeMap<String, CVariant>` で保持する。
//!   `Read`/`Write`(`IPropertyBag` 実装部分)の挙動のみを一致させた
//!   素の Rust 構造体であり、COM の `IPropertyBag` 配線
//!   (`QueryInterface`/`AddRef`/`Release`、`ComUtility.cpp:164-179`)と
//!   `CIUnknownImpl` の参照カウント(`ComUtility.h:39-57`)は対象外。
//!
//! 対象外:
//! - `SafeRelease`(`ComUtility.h:31-37`)。Rust では windows-rs のスマートポインタが
//!   参照カウントを管理するため不要。
//! - `CIUnknownImpl`(`ComUtility.h:39-57`)。COM の参照カウント実装。
//! - `CPropertyPageSite` / `CPropertyPageFrame` / `ShowPropertyPageFrame`
//!   (`ComUtility.cpp:224-568`)。COM プロパティページのダイアログ UI。
//!
//! ## 原実装との差異
//!
//! - 原実装の `CVariant::FromString`(`ComUtility.cpp:149-159`)は `SysAllocString`
//!   失敗時に `E_OUTOFMEMORY` を返すが、windows-rs の `BSTR::from_wide` は確保失敗時に
//!   panic するため、本移植の [`CVariant::from_wide`] が `Err` を返すことは実際にはない
//!   (シグネチャは原実装の HRESULT 戻り値に合わせて `Result` のままとする)。
//! - 原実装の `ToString` は `std::wstring`(UTF-16)へ格納するため、UTF-16 列を
//!   そのまま返す [`CVariant::to_wide`] が厳密な対応物。[`CVariant::to_string`] は
//!   その結果を `String::from_utf16_lossy` で UTF-8 へ変換する便宜メソッド。
//! - `E_POINTER` を返す null ポインタ検査(`ComUtility.cpp:131-132,184-185,200-201`)は
//!   Rust では参照型で表現するため存在しない。

#![cfg(windows)]

use std::collections::btree_map;
use std::collections::BTreeMap;
use std::mem::ManuallyDrop;

use windows::core::BSTR;
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::System::Variant::{
    VariantChangeType, VariantClear, VariantCopy, VariantInit, VARENUM, VARIANT, VARIANT_0_0,
    VARIANT_0_0_0, VARIANT_ALPHABOOL, VT_BSTR, VT_EMPTY,
};

/// `CVariant`(`ComUtility.h:59-77`)。`VARIANT` の RAII ラッパー。
///
/// 原実装は `VARIANT` を public 継承するが、Rust では `VARIANT` を内包し
/// [`CVariant::as_raw`] / [`CVariant::as_raw_mut`] で生の `VARIANT` へアクセスする。
///
/// - 構築(`ComUtility.cpp:36-39`): `VariantInit`(`vt` は `VT_EMPTY`)
/// - 破棄(`ComUtility.cpp:63-66`): `VariantClear`
/// - コピー([`Clone`]、`ComUtility.cpp:42-46`): `VariantCopy` による deep copy
/// - ムーブ(`ComUtility.cpp:56-60`): Rust の所有権移動そのもの
///   ([`From<VARIANT>`] で生 `VARIANT` の所有権を引き取れる)
pub struct CVariant {
    var: VARIANT,
}

impl CVariant {
    /// 既定コンストラクタ(`ComUtility.cpp:36-39`)。`VariantInit` で初期化し、
    /// `vt` は `VT_EMPTY` になる。
    #[must_use]
    pub fn new() -> Self {
        Self {
            var: unsafe { VariantInit() },
        }
    }

    /// `Assign`(`ComUtility.cpp:111-114`)。`VariantCopy` で `var` の内容を
    /// deep copy して自身へ格納する(既存の内容は `VariantCopy` が解放する)。
    ///
    /// # Safety
    /// `var` は正当に初期化された `VARIANT` であること(`vt` と値が整合しており
    /// `VariantCopy` のコピー元にできる状態)。
    pub unsafe fn assign(&mut self, var: &VARIANT) -> windows::core::Result<()> {
        unsafe { VariantCopy(&mut self.var, var) }
    }

    /// `Clear`(`ComUtility.cpp:117-120`)。`VariantClear` で内容を解放し
    /// `VT_EMPTY` に戻す。原実装同様、戻り値(HRESULT)は無視する。
    pub fn clear(&mut self) {
        let _ = unsafe { VariantClear(&mut self.var) };
    }

    /// `ChangeType`(`ComUtility.cpp:123-126`)。
    /// `VariantChangeType(this, this, VARIANT_ALPHABOOL, vt)` で自身を
    /// in-place に型変換する。変換できない場合は元の内容を保ったまま
    /// エラー(`DISP_E_TYPEMISMATCH` 等)を返す。
    pub fn change_type(&mut self, vt: VARENUM) -> windows::core::Result<()> {
        let this: *mut VARIANT = &mut self.var;
        unsafe { VariantChangeType(this, this, VARIANT_ALPHABOOL, vt) }
    }

    /// `ToString`(`ComUtility.cpp:129-146`)の厳密な対応物。一時 `VARIANT` へ
    /// `VariantChangeType(VARIANT_ALPHABOOL, VT_BSTR)` で変換した結果の BSTR を
    /// UTF-16 コード単位列として返す。自身は変更されない。
    ///
    /// `VARIANT_ALPHABOOL` の効果により `VT_BOOL` は "True"/"False" になる。
    pub fn to_wide(&self) -> windows::core::Result<Vec<u16>> {
        let mut tmp = Self::new();
        unsafe {
            VariantChangeType(&mut tmp.var, &self.var, VARIANT_ALPHABOOL, VT_BSTR)?;
        }
        // VT_BSTR へ変換済みの一時 VARIANT から BSTR を参照(所有権は tmp のまま。
        // tmp の Drop = VariantClear が解放する)。
        let wide = unsafe { tmp.var.Anonymous.Anonymous.Anonymous.bstrVal.to_vec() };
        Ok(wide)
    }

    /// `ToString`(`ComUtility.cpp:129-146`)。[`CVariant::to_wide`] の結果を
    /// `String::from_utf16_lossy` で UTF-8 の `String` へ変換して返す。
    /// 自身は変更されない。
    pub fn to_string(&self) -> windows::core::Result<String> {
        Ok(String::from_utf16_lossy(&self.to_wide()?))
    }

    /// `FromString`(`ComUtility.cpp:149-159`)。`VariantClear` 後、`s` から
    /// BSTR を確保して `VT_BSTR` として格納する。
    ///
    /// 原実装は `Str.c_str()` を `SysAllocString` へ渡す(`ComUtility.cpp:153`)ため
    /// 最初の NUL 文字で打ち切られる。本移植も同じ挙動とする。
    /// 確保失敗時の `E_OUTOFMEMORY`(`ComUtility.cpp:154-155`)は windows-rs の
    /// `BSTR::from_wide` が panic するため `Err` としては返らない(クレート概要参照)。
    pub fn from_wide(&mut self, s: &[u16]) -> windows::core::Result<()> {
        self.clear();

        // SysAllocString(c_str()) 相当: 最初の NUL で打ち切る。
        let len = s.iter().position(|&c| c == 0).unwrap_or(s.len());
        let bstr = BSTR::from_wide(&s[..len]);

        // union フィールドへの直接代入(全体を書き込むため安全)。
        self.var.Anonymous.Anonymous = ManuallyDrop::new(VARIANT_0_0 {
            vt: VT_BSTR,
            wReserved1: 0,
            wReserved2: 0,
            wReserved3: 0,
            Anonymous: VARIANT_0_0_0 {
                bstrVal: ManuallyDrop::new(bstr),
            },
        });

        Ok(())
    }

    /// `FromString`(`ComUtility.cpp:149-159`)。`&str` を UTF-16 へ変換して
    /// [`CVariant::from_wide`] を呼ぶ便宜メソッド。
    pub fn from_string(&mut self, s: &str) -> windows::core::Result<()> {
        let wide: Vec<u16> = s.encode_utf16().collect();
        self.from_wide(&wide)
    }

    /// 現在の型(`VARIANT::vt`)を返す。
    #[must_use]
    pub fn vt(&self) -> VARENUM {
        unsafe { self.var.Anonymous.Anonymous.vt }
    }

    /// 生の `VARIANT` への参照。COM API へ `*const VARIANT` として渡せる。
    #[must_use]
    pub fn as_raw(&self) -> &VARIANT {
        &self.var
    }

    /// 生の `VARIANT` への可変参照。COM API へ `*mut VARIANT` として渡せる。
    ///
    /// # Safety
    /// 呼び出し側は `VARIANT` を「`VariantClear` に渡せる正当な状態」
    /// (`vt` と保持する値・ポインタが整合した状態)に保たなければならない。
    /// 不整合な状態のまま `CVariant` を破棄すると `VariantClear` が未定義動作を起こす。
    pub unsafe fn as_raw_mut(&mut self) -> &mut VARIANT {
        &mut self.var
    }

    /// 内包する `VARIANT` の所有権を取り出す(`VariantClear` は呼ばれない)。
    /// 取り出した `VARIANT` の解放責任は呼び出し側へ移る。
    #[must_use]
    pub fn into_raw(self) -> VARIANT {
        let this = ManuallyDrop::new(self);
        unsafe { std::ptr::read(&this.var) }
    }
}

impl Default for CVariant {
    /// 既定コンストラクタ(`ComUtility.cpp:36-39`)。
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for CVariant {
    /// デストラクタ(`ComUtility.cpp:63-66`)。`VariantClear` で内容を解放する。
    fn drop(&mut self) {
        let _ = unsafe { VariantClear(&mut self.var) };
    }
}

impl Clone for CVariant {
    /// コピーコンストラクタ(`ComUtility.cpp:42-46`)。`VariantCopy` による
    /// deep copy(BSTR 等は複製され、元と独立になる)。
    ///
    /// 原実装同様 `VariantCopy` の HRESULT は無視される(コピー失敗時は
    /// `VT_EMPTY` のままになる)。失敗を検出したい場合は
    /// [`TryFrom<&VARIANT>`] を使うこと。
    fn clone(&self) -> Self {
        let mut var = Self::new();
        let _ = unsafe { var.assign(&self.var) };
        var
    }
}

impl From<VARIANT> for CVariant {
    /// ムーブコンストラクタ(`ComUtility.cpp:56-60`)。生 `VARIANT` の所有権を
    /// 引き取る(コピーは発生せず、以後の解放は `CVariant` が行う)。
    /// 原実装がムーブ元を `VariantInit` で空に戻す処理は、Rust では所有権の
    /// 移動そのものにあたるため不要。
    fn from(var: VARIANT) -> Self {
        Self { var }
    }
}

impl TryFrom<&VARIANT> for CVariant {
    type Error = windows::core::Error;

    /// コピーコンストラクタ(`ComUtility.cpp:49-53`)の失敗を検出できる版。
    /// `VariantCopy` による deep copy を行い、失敗時はエラーを返す。
    fn try_from(var: &VARIANT) -> windows::core::Result<Self> {
        let mut v = Self::new();
        unsafe { v.assign(var)? };
        Ok(v)
    }
}

/// `CPropertyBag`(`ComUtility.h:79-103`、`ComUtility.cpp:164-219`)。
/// キー文字列 → [`CVariant`] のプロパティ管理。
///
/// 原実装は `IPropertyBag` を実装する COM オブジェクトだが、本移植は
/// `Read`/`Write` の挙動のみを一致させた素の Rust 構造体であり、COM の
/// `IPropertyBag` 配線(`QueryInterface`/`AddRef`/`Release`)は対象外。
/// キーは原実装の `std::map<String, CVariant>`(`ComUtility.h:84`)と同じく
/// 大文字小文字を区別し、辞書順で整列される。
#[derive(Default)]
pub struct CPropertyBag {
    properties: BTreeMap<String, CVariant>,
}

impl CPropertyBag {
    /// 空のプロパティバッグを生成する。
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// `Read`(`ComUtility.cpp:182-195`)。プロパティ `name` の値を `var` へ返す。
    ///
    /// - キーが存在しなければ `E_INVALIDARG`(`ComUtility.cpp:188-189`)。
    /// - `var` の `vt` が `VT_EMPTY` または格納値と同じ `vt` なら `VariantCopy` で
    ///   そのまま返す(`ComUtility.cpp:191-192`)。
    /// - それ以外は `VariantChangeType(VARIANT_ALPHABOOL)` で `var` の `vt` が
    ///   要求する型へ変換して返す(`ComUtility.cpp:194`)。変換できなければ
    ///   `DISP_E_TYPEMISMATCH` 等を返す。
    ///
    /// # Safety
    /// `var` は正当に初期化された `VARIANT` であること(`VariantCopy` /
    /// `VariantChangeType` のコピー先として既存内容が正しく解放できる状態)。
    pub unsafe fn read(&self, name: &str, var: &mut VARIANT) -> windows::core::Result<()> {
        let Some(value) = self.properties.get(name) else {
            return Err(E_INVALIDARG.into());
        };

        let requested = unsafe { var.Anonymous.Anonymous.vt };
        if requested == VT_EMPTY || requested == value.vt() {
            unsafe { VariantCopy(var, value.as_raw()) }
        } else {
            unsafe { VariantChangeType(var, value.as_raw(), VARIANT_ALPHABOOL, requested) }
        }
    }

    /// `Write`(`ComUtility.cpp:198-219`)。プロパティ `name` へ `var` の deep copy を
    /// 格納する。既存キーは上書きされる(`ComUtility.cpp:208-216`)。
    /// `VariantCopy` が失敗した場合はそのエラーを返し、バッグは変更されない
    /// (`ComUtility.cpp:203-206`)。
    ///
    /// # Safety
    /// `var` は正当に初期化された `VARIANT` であること(`VariantCopy` の
    /// コピー元にできる状態)。
    pub unsafe fn write(&mut self, name: &str, var: &VARIANT) -> windows::core::Result<()> {
        let mut value = CVariant::new();
        unsafe { value.assign(var)? };
        self.properties.insert(name.to_owned(), value);
        Ok(())
    }

    /// `begin`/`end`(`ComUtility.h:96-97`)。プロパティをキーの辞書順で列挙する
    /// イテレータを返す。
    pub fn iter(&self) -> btree_map::Iter<'_, String, CVariant> {
        self.properties.iter()
    }

    /// 保持しているプロパティ数を返す。
    #[must_use]
    pub fn len(&self) -> usize {
        self.properties.len()
    }

    /// プロパティが 1 つもなければ `true`。
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.properties.is_empty()
    }
}

impl<'a> IntoIterator for &'a CPropertyBag {
    type Item = (&'a String, &'a CVariant);
    type IntoIter = btree_map::Iter<'a, String, CVariant>;

    /// `begin`/`end`(`ComUtility.h:96-97`)相当。
    fn into_iter(self) -> Self::IntoIter {
        self.properties.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{
        DISP_E_BADVARTYPE, DISP_E_TYPEMISMATCH, VARIANT_FALSE, VARIANT_TRUE,
    };
    use windows::Win32::System::Variant::{VT_BOOL, VT_I4, VT_R8};

    /// テスト用: 指定した vt と値を持つ生 VARIANT を組み立てる。
    fn make_variant(vt: VARENUM, value: VARIANT_0_0_0) -> VARIANT {
        let mut var = unsafe { VariantInit() };
        var.Anonymous.Anonymous = ManuallyDrop::new(VARIANT_0_0 {
            vt,
            wReserved1: 0,
            wReserved2: 0,
            wReserved3: 0,
            Anonymous: value,
        });
        var
    }

    fn make_i4(value: i32) -> VARIANT {
        make_variant(VT_I4, VARIANT_0_0_0 { lVal: value })
    }

    fn make_bool(value: bool) -> VARIANT {
        make_variant(
            VT_BOOL,
            VARIANT_0_0_0 {
                boolVal: if value { VARIANT_TRUE } else { VARIANT_FALSE },
            },
        )
    }

    fn i4_value(var: &VARIANT) -> i32 {
        unsafe { var.Anonymous.Anonymous.Anonymous.lVal }
    }

    fn r8_value(var: &VARIANT) -> f64 {
        unsafe { var.Anonymous.Anonymous.Anonymous.dblVal }
    }

    // ---- CVariant ----

    #[test]
    fn new_is_vt_empty() {
        let var = CVariant::new();
        assert_eq!(var.vt(), VT_EMPTY);
        assert_eq!(CVariant::default().vt(), VT_EMPTY);
    }

    #[test]
    fn i4_to_string() {
        let var = CVariant::from(make_i4(123));
        assert_eq!(var.to_string().unwrap(), "123");
    }

    #[test]
    fn bool_to_string_is_alphabool() {
        // VARIANT_ALPHABOOL の効果で "True"/"False" になる("-1"/"0" ではない)。
        let t = CVariant::from(make_bool(true));
        assert_eq!(t.to_string().unwrap(), "True");
        let f = CVariant::from(make_bool(false));
        assert_eq!(f.to_string().unwrap(), "False");
    }

    #[test]
    fn to_string_does_not_modify_self() {
        // ToString(ComUtility.cpp:129-146)は一時 VARIANT を使い、自身は const。
        let var = CVariant::from(make_i4(123));
        let _ = var.to_string().unwrap();
        assert_eq!(var.vt(), VT_I4);
        assert_eq!(i4_value(var.as_raw()), 123);
    }

    #[test]
    fn empty_to_string_is_empty() {
        let var = CVariant::new();
        assert_eq!(var.to_string().unwrap(), "");
        assert_eq!(var.to_wide().unwrap(), Vec::<u16>::new());
    }

    #[test]
    fn from_string_roundtrip() {
        let mut var = CVariant::new();
        var.from_string("Hello").unwrap();
        assert_eq!(var.vt(), VT_BSTR);
        assert_eq!(var.to_string().unwrap(), "Hello");
    }

    #[test]
    fn from_wide_roundtrip() {
        let wide: Vec<u16> = "テスト".encode_utf16().collect();
        let mut var = CVariant::new();
        var.from_wide(&wide).unwrap();
        assert_eq!(var.vt(), VT_BSTR);
        assert_eq!(var.to_wide().unwrap(), wide);
        assert_eq!(var.to_string().unwrap(), "テスト");
    }

    #[test]
    fn from_wide_truncates_at_nul() {
        // 原実装は SysAllocString(Str.c_str()) のため最初の NUL で打ち切られる。
        let wide = [u16::from(b'a'), 0, u16::from(b'b')];
        let mut var = CVariant::new();
        var.from_wide(&wide).unwrap();
        assert_eq!(var.to_wide().unwrap(), vec![u16::from(b'a')]);
    }

    #[test]
    fn from_string_replaces_previous_value() {
        // FromString は VariantClear 後に設定する(ComUtility.cpp:151)。
        let mut var = CVariant::from(make_i4(42));
        var.from_string("abc").unwrap();
        assert_eq!(var.vt(), VT_BSTR);
        assert_eq!(var.to_string().unwrap(), "abc");
    }

    #[test]
    fn change_type_i4_to_r8() {
        let mut var = CVariant::from(make_i4(123));
        var.change_type(VT_R8).unwrap();
        assert_eq!(var.vt(), VT_R8);
        assert!((r8_value(var.as_raw()) - 123.0).abs() < f64::EPSILON);
    }

    #[test]
    fn change_type_bstr_to_i4() {
        let mut var = CVariant::new();
        var.from_string("456").unwrap();
        var.change_type(VT_I4).unwrap();
        assert_eq!(var.vt(), VT_I4);
        assert_eq!(i4_value(var.as_raw()), 456);
    }

    #[test]
    fn change_type_alphabool_string_to_bool() {
        // VARIANT_ALPHABOOL により "True" → VARIANT_TRUE の変換が効く。
        let mut var = CVariant::new();
        var.from_string("True").unwrap();
        var.change_type(VT_BOOL).unwrap();
        assert_eq!(var.vt(), VT_BOOL);
        let value = unsafe { var.as_raw().Anonymous.Anonymous.Anonymous.boolVal };
        assert_eq!(value, VARIANT_TRUE);
    }

    #[test]
    fn change_type_mismatch_fails() {
        let mut var = CVariant::new();
        var.from_string("abc").unwrap();
        let err = var.change_type(VT_I4).unwrap_err();
        assert_eq!(err.code(), DISP_E_TYPEMISMATCH);
        // 変換失敗時は元の内容が保たれる。
        assert_eq!(var.vt(), VT_BSTR);
        assert_eq!(var.to_string().unwrap(), "abc");
    }

    #[test]
    fn clone_is_deep_copy() {
        // clone 後に元を clear しても複製が生きる = BSTR の deep copy。
        let mut original = CVariant::new();
        original.from_string("Hello").unwrap();
        let cloned = original.clone();
        original.clear();
        assert_eq!(original.vt(), VT_EMPTY);
        assert_eq!(cloned.vt(), VT_BSTR);
        assert_eq!(cloned.to_string().unwrap(), "Hello");
    }

    #[test]
    fn assign_is_deep_copy() {
        let mut source = CVariant::new();
        source.from_string("World").unwrap();
        let mut target = CVariant::new();
        unsafe { target.assign(source.as_raw()) }.unwrap();
        source.clear();
        assert_eq!(target.to_string().unwrap(), "World");
    }

    #[test]
    fn try_from_variant_is_deep_copy() {
        let mut source = CVariant::new();
        source.from_string("copy").unwrap();
        let copied = CVariant::try_from(source.as_raw()).unwrap();
        source.clear();
        assert_eq!(copied.vt(), VT_BSTR);
        assert_eq!(copied.to_string().unwrap(), "copy");
    }

    #[test]
    fn from_variant_takes_ownership() {
        // ムーブコンストラクタ(ComUtility.cpp:56-60)相当: コピーせず所有権を引き取る。
        let raw = make_i4(789);
        let var = CVariant::from(raw);
        assert_eq!(var.vt(), VT_I4);
        assert_eq!(var.to_string().unwrap(), "789");
    }

    #[test]
    fn into_raw_detaches_ownership() {
        let mut var = CVariant::new();
        var.from_string("detach").unwrap();
        let raw = var.into_raw();
        // 解放責任ごと引き取ったので CVariant に戻して破棄させる。
        let var = CVariant::from(raw);
        assert_eq!(var.to_string().unwrap(), "detach");
    }

    #[test]
    fn clear_resets_to_empty() {
        let mut var = CVariant::from(make_i4(1));
        var.clear();
        assert_eq!(var.vt(), VT_EMPTY);
    }

    // ---- CPropertyBag ----

    #[test]
    fn read_unknown_key_is_e_invalidarg() {
        let bag = CPropertyBag::new();
        let mut out = CVariant::new();
        let err = unsafe { bag.read("missing", out.as_raw_mut()) }.unwrap_err();
        assert_eq!(err.code(), E_INVALIDARG);
    }

    #[test]
    fn read_with_vt_empty_returns_stored_type() {
        let mut bag = CPropertyBag::new();
        unsafe { bag.write("value", &make_i4(123)) }.unwrap();

        let mut out = CVariant::new();
        assert_eq!(out.vt(), VT_EMPTY);
        unsafe { bag.read("value", out.as_raw_mut()) }.unwrap();
        assert_eq!(out.vt(), VT_I4);
        assert_eq!(i4_value(out.as_raw()), 123);
    }

    #[test]
    fn read_with_same_type_copies() {
        let mut bag = CPropertyBag::new();
        let mut stored = CVariant::new();
        stored.from_string("new").unwrap();
        unsafe { bag.write("name", stored.as_raw()) }.unwrap();

        // 読み出し先が既に同じ vt(VT_BSTR)の値を保持していても正しく上書きされる。
        let mut out = CVariant::new();
        out.from_string("old").unwrap();
        unsafe { bag.read("name", out.as_raw_mut()) }.unwrap();
        assert_eq!(out.vt(), VT_BSTR);
        assert_eq!(out.to_string().unwrap(), "new");
    }

    #[test]
    fn read_converts_to_requested_type() {
        let mut bag = CPropertyBag::new();
        unsafe { bag.write("number", &make_i4(123)) }.unwrap();
        unsafe { bag.write("flag", &make_bool(true)) }.unwrap();

        // VT_I4 で格納した値を VT_BSTR として要求 → "123" に変換される。
        let mut out = CVariant::new();
        out.from_string("").unwrap();
        assert_eq!(out.vt(), VT_BSTR);
        unsafe { bag.read("number", out.as_raw_mut()) }.unwrap();
        assert_eq!(out.vt(), VT_BSTR);
        assert_eq!(out.to_string().unwrap(), "123");

        // VT_BOOL で格納した値を VT_BSTR として要求 → VARIANT_ALPHABOOL で "True"。
        let mut out = CVariant::new();
        out.from_string("").unwrap();
        unsafe { bag.read("flag", out.as_raw_mut()) }.unwrap();
        assert_eq!(out.to_string().unwrap(), "True");
    }

    #[test]
    fn read_conversion_failure_is_type_mismatch() {
        let mut bag = CPropertyBag::new();
        let mut stored = CVariant::new();
        stored.from_string("abc").unwrap();
        unsafe { bag.write("text", stored.as_raw()) }.unwrap();

        let mut out = CVariant::from(make_i4(0));
        let err = unsafe { bag.read("text", out.as_raw_mut()) }.unwrap_err();
        assert_eq!(err.code(), DISP_E_TYPEMISMATCH);
        // 格納値は変化しない。
        let mut check = CVariant::new();
        unsafe { bag.read("text", check.as_raw_mut()) }.unwrap();
        assert_eq!(check.to_string().unwrap(), "abc");
    }

    #[test]
    fn write_overwrites_existing_key() {
        let mut bag = CPropertyBag::new();
        unsafe { bag.write("key", &make_i4(123)) }.unwrap();
        unsafe { bag.write("key", &make_i4(456)) }.unwrap();
        assert_eq!(bag.len(), 1);

        let mut out = CVariant::new();
        unsafe { bag.read("key", out.as_raw_mut()) }.unwrap();
        assert_eq!(i4_value(out.as_raw()), 456);
    }

    #[test]
    fn write_copy_failure_leaves_bag_unchanged() {
        // 不正な vt を持つ VARIANT は VariantCopy が DISP_E_BADVARTYPE を返す。
        let mut bag = CPropertyBag::new();
        let invalid = make_variant(VARENUM(0x7FFF), VARIANT_0_0_0 { llVal: 0 });
        let err = unsafe { bag.write("bad", &invalid) }.unwrap_err();
        assert_eq!(err.code(), DISP_E_BADVARTYPE);
        assert!(bag.is_empty());
        let mut out = CVariant::new();
        let err = unsafe { bag.read("bad", out.as_raw_mut()) }.unwrap_err();
        assert_eq!(err.code(), E_INVALIDARG);
    }

    #[test]
    fn write_stores_deep_copy() {
        let mut bag = CPropertyBag::new();
        let mut source = CVariant::new();
        source.from_string("independent").unwrap();
        unsafe { bag.write("s", source.as_raw()) }.unwrap();
        source.clear();

        let mut out = CVariant::new();
        unsafe { bag.read("s", out.as_raw_mut()) }.unwrap();
        assert_eq!(out.to_string().unwrap(), "independent");
    }

    #[test]
    fn keys_are_case_sensitive() {
        let mut bag = CPropertyBag::new();
        unsafe { bag.write("Key", &make_i4(1)) }.unwrap();
        unsafe { bag.write("key", &make_i4(2)) }.unwrap();
        assert_eq!(bag.len(), 2);

        let mut out = CVariant::new();
        unsafe { bag.read("Key", out.as_raw_mut()) }.unwrap();
        assert_eq!(i4_value(out.as_raw()), 1);
        let mut out = CVariant::new();
        unsafe { bag.read("key", out.as_raw_mut()) }.unwrap();
        assert_eq!(i4_value(out.as_raw()), 2);
    }

    #[test]
    fn iter_is_sorted_by_key() {
        // std::map と同様、キーの辞書順で列挙される。
        let mut bag = CPropertyBag::new();
        unsafe { bag.write("b", &make_i4(2)) }.unwrap();
        unsafe { bag.write("a", &make_i4(1)) }.unwrap();
        unsafe { bag.write("c", &make_i4(3)) }.unwrap();

        let keys: Vec<&str> = bag.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["a", "b", "c"]);

        let values: Vec<i32> = (&bag)
            .into_iter()
            .map(|(_, v)| i4_value(v.as_raw()))
            .collect();
        assert_eq!(values, [1, 2, 3]);
    }
}
