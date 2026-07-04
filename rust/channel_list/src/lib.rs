// TVTest の ChannelList.cpp を Rust へ移植したもの。
//
// 移植対象:
//   - ChannelInfo              : CChannelInfo (ChannelList.h:37)
//   - TunerChannelInfo         : CTunerChannelInfo (ChannelList.h:77)
//   - ChannelList              : CChannelList (ChannelList.h:92)
//   - TuningSpaceInfo          : CTuningSpaceInfo (ChannelList.h:145)
//   - TuningSpaceList          : CTuningSpaceList (ChannelList.h:177)
//   - parse_channel_csv        : CTuningSpaceList::LoadFromFile の CSV パース純粋部分
//   - serialize_channel_csv    : CTuningSpaceList::SaveToFile  の CSV シリアライズ純粋部分
//
// SaveToFile/LoadFromFile のファイル I/O・文字コード変換(Win32)は対象外。
// SetName の StrStr/StrStrI は str::contains で代替。
// SortType::Name の lstrcmpi は ASCII 大小無視比較で近似。
//
// 文字列は内部的に String(UTF-8)で保持。原実装の wchar_t(UTF-16)と
// 異なるが、チャンネル名は BMP 内文字のみの運用が前提のため等価。

use std::cmp::Ordering;

pub const FIRST_UHF_CHANNEL: i32 = 13;
pub const MAX_CHANNEL_NAME: usize = 64;

/// チャンネル情報。原実装 CChannelInfo (ChannelList.h:37)。
#[derive(Debug, Clone, Default)]
pub struct ChannelInfo {
    pub space: i32,
    pub channel_index: i32,
    pub channel_no: i32,
    pub physical_channel: i32,
    pub name: String,
    pub network_id: u16,
    pub transport_stream_id: u16,
    pub service_id: u16,
    pub service_type: u8,
    pub enabled: bool,
}

impl ChannelInfo {
    pub fn new(space: i32, channel_index: i32, no: i32, name: &str) -> Self {
        ChannelInfo {
            space,
            channel_index,
            channel_no: no,
            physical_channel: 0,
            name: name.to_string(),
            network_id: 0,
            transport_stream_id: 0,
            service_id: 0,
            service_type: 0,
            enabled: true,
        }
    }

    pub fn set_channel_no(&mut self, no: i32) -> bool {
        if no < 0 {
            return false;
        }
        self.channel_no = no;
        true
    }
}

/// チューナーチャンネル情報。原実装 CTunerChannelInfo (ChannelList.h:77)。
#[derive(Debug, Clone, Default)]
pub struct TunerChannelInfo {
    pub info: ChannelInfo,
    pub tuner_name: String,
}

/// チャンネルリストのソート種別。原実装 CChannelList::SortType (ChannelList.h:95)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortType {
    Space,
    ChannelIndex,
    ChannelNo,
    PhysicalChannel,
    Name,
    NetworkId,
    ServiceId,
}

/// チャンネルリスト。原実装 CChannelList (ChannelList.h:92)。
#[derive(Debug, Clone, Default)]
pub struct ChannelList {
    channels: Vec<ChannelInfo>,
}

impl ChannelList {
    pub fn new() -> Self {
        ChannelList { channels: Vec::new() }
    }

    /// 原実装 NumChannels:111。
    pub fn num_channels(&self) -> usize {
        self.channels.len()
    }

    /// 原実装 NumEnableChannels:151。
    pub fn num_enabled_channels(&self) -> usize {
        self.channels.iter().filter(|c| c.enabled).count()
    }

    /// 原実装 AddChannel:163。
    pub fn add_channel(&mut self, info: ChannelInfo) {
        self.channels.push(info);
    }

    /// 原実装 InsertChannel:178。
    pub fn insert_channel(&mut self, index: usize, info: ChannelInfo) -> bool {
        if index > self.channels.len() {
            return false;
        }
        self.channels.insert(index, info);
        true
    }

    /// 原実装 GetChannelInfo:194。
    pub fn get_channel(&self, index: usize) -> Option<&ChannelInfo> {
        self.channels.get(index)
    }

    pub fn get_channel_mut(&mut self, index: usize) -> Option<&mut ChannelInfo> {
        self.channels.get_mut(index)
    }

    /// 原実装 DeleteChannel:263。
    pub fn delete_channel(&mut self, index: usize) -> bool {
        if index >= self.channels.len() {
            return false;
        }
        self.channels.remove(index);
        true
    }

    pub fn clear(&mut self) {
        self.channels.clear();
    }

    /// ポインタ一致検索は Rust では不要。value 一致版のみ実装。
    /// 原実装 Find(CChannelInfo&,bool):293。
    pub fn find(&self, info: &ChannelInfo, enabled_only: bool) -> Option<usize> {
        self.channels.iter().position(|ch| {
            if enabled_only && !ch.enabled {
                return false;
            }
            (info.space < 0 || ch.space == info.space)
                && (info.channel_index < 0 || ch.channel_index == info.channel_index)
                && (info.channel_no <= 0 || ch.channel_no == info.channel_no)
                && (info.physical_channel <= 0 || ch.physical_channel == info.physical_channel)
                && (info.network_id == 0 || ch.network_id == info.network_id)
                && (info.transport_stream_id == 0
                    || ch.transport_stream_id == info.transport_stream_id)
                && (info.service_id == 0 || ch.service_id == info.service_id)
        })
    }

    /// 原実装 FindByIndex:319。
    pub fn find_by_index(
        &self,
        space: i32,
        channel_index: i32,
        service_id: i32,
        enabled_only: bool,
    ) -> Option<usize> {
        self.channels.iter().position(|ch| {
            (!enabled_only || ch.enabled)
                && (space < 0 || ch.space == space)
                && (channel_index < 0 || ch.channel_index == channel_index)
                && (service_id <= 0 || ch.service_id as i32 == service_id)
        })
    }

    /// 原実装 FindPhysicalChannel:334。
    pub fn find_physical_channel(&self, channel: i32) -> Option<usize> {
        self.channels.iter().position(|ch| ch.physical_channel == channel)
    }

    /// 原実装 FindChannelNo:344。
    pub fn find_channel_no(&self, no: i32, enabled_only: bool) -> Option<usize> {
        self.channels.iter().position(|ch| {
            ch.channel_no == no && (!enabled_only || ch.enabled)
        })
    }

    /// 原実装 FindServiceID:356。
    pub fn find_service_id(&self, service_id: u16) -> Option<usize> {
        self.channels.iter().position(|ch| ch.service_id == service_id)
    }

    /// 原実装 FindByIDs:366。
    pub fn find_by_ids(
        &self,
        network_id: u16,
        transport_stream_id: u16,
        service_id: u16,
        enabled_only: bool,
    ) -> Option<usize> {
        self.channels.iter().position(|ch| {
            (!enabled_only || ch.enabled)
                && (network_id == 0 || ch.network_id == network_id)
                && (transport_stream_id == 0 || ch.transport_stream_id == transport_stream_id)
                && (service_id == 0 || ch.service_id == service_id)
        })
    }

    /// 原実装 FindByName:380。
    pub fn find_by_name(&self, name: &str) -> Option<usize> {
        self.channels.iter().position(|ch| ch.name == name)
    }

    /// 原実装 GetNextChannel:392。次の有効チャンネル(リモコン番号順)を返す。
    pub fn get_next_channel(&self, index: usize, wrap: bool) -> Option<usize> {
        if index >= self.channels.len() {
            return None;
        }
        let ch_no = self.channels[index].channel_no;

        // 同じリモコン番号を後方で検索。
        for i in (index + 1)..self.channels.len() {
            let ch = &self.channels[i];
            if ch.enabled && ch.channel_no == ch_no {
                return Some(i);
            }
        }

        // 次に大きいリモコン番号を検索。
        let mut next_no = i32::MAX;
        let mut min_no = i32::MAX;
        for ch in &self.channels {
            if ch.enabled && ch.channel_no != 0 {
                if ch.channel_no > ch_no && ch.channel_no < next_no {
                    next_no = ch.channel_no;
                }
                if ch.channel_no < min_no {
                    min_no = ch.channel_no;
                }
            }
        }
        if next_no == i32::MAX {
            if min_no == i32::MAX || !wrap {
                return None;
            }
            next_no = min_no;
        }
        self.find_channel_no(next_no, true)
    }

    /// 原実装 GetPrevChannel:428。
    pub fn get_prev_channel(&self, index: usize, wrap: bool) -> Option<usize> {
        if index >= self.channels.len() {
            return None;
        }
        let ch_no = self.channels[index].channel_no;

        // 同じリモコン番号を前方で検索。
        for i in (0..index).rev() {
            let ch = &self.channels[i];
            if ch.enabled && ch.channel_no == ch_no {
                return Some(i);
            }
        }

        // 次に小さいリモコン番号を検索。
        let mut prev_no = 0i32;
        let mut max_no = 0i32;
        for ch in &self.channels {
            if ch.enabled && ch.channel_no != 0 {
                if ch.channel_no < ch_no && ch.channel_no > prev_no {
                    prev_no = ch.channel_no;
                }
                if ch.channel_no > max_no {
                    max_no = ch.channel_no;
                }
            }
        }
        if prev_no == 0 {
            if !wrap {
                return None;
            }
            prev_no = max_no;
        }

        // 最後尾から逆順に検索して最後の一致を返す。
        for i in (0..self.channels.len()).rev() {
            let ch = &self.channels[i];
            if ch.enabled && ch.channel_no == prev_no {
                return Some(i);
            }
        }
        None
    }

    /// 原実装 GetMaxChannelNo:472。
    pub fn get_max_channel_no(&self) -> i32 {
        self.channels.iter().map(|ch| ch.channel_no).max().unwrap_or(0)
    }

    /// 原実装 Sort:485。
    pub fn sort(&mut self, sort_type: SortType, descending: bool) {
        self.channels.sort_by(|a, b| {
            let cmp = match sort_type {
                SortType::Space => a.space.cmp(&b.space),
                SortType::ChannelIndex => a.channel_index.cmp(&b.channel_index),
                SortType::ChannelNo => a.channel_no.cmp(&b.channel_no),
                SortType::PhysicalChannel => a.physical_channel.cmp(&b.physical_channel),
                SortType::Name => {
                    // lstrcmpi 相当: ASCII 大小無視比較。
                    let ci = a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase());
                    if ci == Ordering::Equal {
                        a.name.cmp(&b.name)
                    } else {
                        ci
                    }
                }
                SortType::NetworkId => a.network_id.cmp(&b.network_id),
                SortType::ServiceId => a.service_id.cmp(&b.service_id),
            };
            if descending {
                cmp.reverse()
            } else {
                cmp
            }
        });
    }

    /// 原実装 HasRemoteControlKeyID:546。
    pub fn has_remote_control_key_id(&self) -> bool {
        self.channels.iter().any(|ch| ch.channel_no != 0)
    }

    /// 原実装 HasMultiService:556。
    pub fn has_multi_service(&self) -> bool {
        for i in 0..self.channels.len() {
            for j in (i + 1)..self.channels.len() {
                let a = &self.channels[i];
                let b = &self.channels[j];
                if a.network_id == b.network_id
                    && a.transport_stream_id == b.transport_stream_id
                    && a.service_id != b.service_id
                {
                    return true;
                }
            }
        }
        false
    }
}

/// チューニング空間種別。原実装 CTuningSpaceInfo::TuningSpaceType (ChannelList.h:148)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuningSpaceType {
    Unknown = 0,
    Terrestrial,
    Bs,
    Cs110,
}

/// チューニング空間情報。原実装 CTuningSpaceInfo (ChannelList.h:145)。
#[derive(Debug, Clone)]
pub struct TuningSpaceInfo {
    pub channel_list: ChannelList,
    pub name: String,
    pub space_type: TuningSpaceType,
}

impl TuningSpaceInfo {
    pub fn new() -> Self {
        TuningSpaceInfo {
            channel_list: ChannelList::new(),
            name: String::new(),
            space_type: TuningSpaceType::Unknown,
        }
    }

    /// 原実装 SetName:617。地上/BS/CS をチューニング空間名から判定。
    /// StrStr/StrStrI(shlwapi) は str::contains で代替。
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
        let lower = name.to_ascii_lowercase();
        if name.contains('地')
            || lower.contains("vhf")
            || lower.contains("uhf")
            || lower.contains("catv")
        {
            self.space_type = TuningSpaceType::Terrestrial;
        } else if lower.contains("bs") {
            self.space_type = TuningSpaceType::Bs;
        } else if lower.contains("cs") {
            self.space_type = TuningSpaceType::Cs110;
        } else {
            self.space_type = TuningSpaceType::Unknown;
        }
    }

    pub fn num_channels(&self) -> usize {
        self.channel_list.num_channels()
    }
}

impl Default for TuningSpaceInfo {
    fn default() -> Self {
        Self::new()
    }
}

/// チューニング空間リスト。原実装 CTuningSpaceList (ChannelList.h:177)。
#[derive(Debug, Clone, Default)]
pub struct TuningSpaceList {
    pub spaces: Vec<TuningSpaceInfo>,
    pub all_channels: ChannelList,
}

impl TuningSpaceList {
    pub fn new() -> Self {
        TuningSpaceList {
            spaces: Vec::new(),
            all_channels: ChannelList::new(),
        }
    }

    pub fn num_spaces(&self) -> usize {
        self.spaces.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spaces.is_empty()
    }

    /// 原実装 Reserve:751。
    pub fn reserve(&mut self, n: usize) {
        if n <= self.spaces.len() {
            self.spaces.truncate(n);
        } else {
            while self.spaces.len() < n {
                self.spaces.push(TuningSpaceInfo::new());
            }
        }
    }

    /// 原実装 MakeTuningSpaceList:721。
    pub fn make_tuning_space_list(&mut self, list: &ChannelList) {
        let max_space = list
            .channels
            .iter()
            .map(|ch| ch.space)
            .max()
            .unwrap_or(-1);
        let needed = (max_space + 1).max(0) as usize;
        self.reserve(needed);

        for ch in &list.channels {
            if ch.space >= 0 {
                self.spaces[ch.space as usize].channel_list.add_channel(ch.clone());
            }
        }
    }

    /// 原実装 Create:741。
    pub fn create(&mut self, list: &ChannelList) {
        self.clear();
        self.make_tuning_space_list(list);
        self.all_channels = list.clone();
    }

    /// 原実装 MakeAllChannelList:782。
    pub fn make_all_channel_list(&mut self) {
        self.all_channels.clear();
        for space in &self.spaces {
            for ch in &space.channel_list.channels {
                self.all_channels.add_channel(ch.clone());
            }
        }
    }

    pub fn clear(&mut self) {
        self.spaces.clear();
        self.all_channels.clear();
    }

    /// CTuningSpaceList::LoadFromFile の CSV パース部分(ファイル I/O を除く純粋ロジック)。
    /// 原実装:978-1170。`text` は UTF-8 テキスト(BOM除去・改行正規化済み)。
    ///
    /// CSV フォーマット:
    ///   名称,チューニング空間,チャンネル[,リモコン番号[,サービスタイプ[,サービスID
    ///         [,ネットワークID[,TSID[,状態]]]]]]
    ///   `#` または `;` で始まる行はコメント。
    ///   `##SPACE(インデックス,名前)` でチューニング空間名を設定。
    pub fn parse_csv(&mut self, text: &str) {
        self.all_channels.clear();

        for line in text.lines() {
            let line = line.trim_matches(|c| c == '\r' || c == '\n').trim();
            if line.is_empty() {
                continue;
            }

            if let Some(rest) = line.strip_prefix('#').or_else(|| line.strip_prefix(';')) {
                // `##SPACE(index,name)` 形式のチューニング空間名。
                if let Some(rest2) = rest.strip_prefix('#') {
                    if let Some(inner) = rest2.strip_prefix("SPACE(") {
                        if let Some(close) = inner.find(')') {
                            let args = &inner[..close];
                            if let Some((idx_str, name)) = args.split_once(',') {
                                let idx_str = idx_str.trim();
                                let name = name.trim();
                                if let Ok(idx) = idx_str.parse::<usize>() {
                                    if idx < 100 && !name.is_empty() {
                                        if self.spaces.len() <= idx {
                                            self.reserve(idx + 1);
                                        }
                                        self.spaces[idx].set_name(name);
                                    }
                                }
                            }
                        }
                    }
                }
                continue;
            }

            if let Some(ch) = parse_channel_line(line) {
                self.all_channels.add_channel(ch);
            }
        }

        // all_channels からチューニング空間別リストを再構築。
        let flat: Vec<ChannelInfo> = self.all_channels.channels.clone();
        let max_space = flat.iter().map(|ch| ch.space).max().unwrap_or(-1);
        if max_space >= 0 {
            let needed = (max_space + 1) as usize;
            if self.spaces.len() < needed {
                self.reserve(needed);
            }
            for ch in &flat {
                if ch.space >= 0 {
                    let idx = ch.space as usize;
                    if idx < self.spaces.len() {
                        // 空間リストは parse_csv 開始時にクリアしていないため、
                        // channel_list だけをリセットしてから追加する。
                        // (SPACE 名は保持)
                    }
                    let _ = idx;
                }
            }
            // 空間別チャンネルリストをリセットして再構築。
            for space in self.spaces.iter_mut() {
                space.channel_list.clear();
            }
            for ch in &flat {
                if ch.space >= 0 {
                    let idx = ch.space as usize;
                    if idx < self.spaces.len() {
                        self.spaces[idx].channel_list.add_channel(ch.clone());
                    }
                }
            }
        }
    }

    /// CTuningSpaceList::SaveToFile の CSV シリアライズ部分(ファイル I/O を除く純粋ロジック)。
    /// 原実装:798-942。
    pub fn serialize_csv(&self) -> String {
        let mut buf = String::new();
        buf.push_str("; TVTest チャンネル設定ファイル\r\n");
        buf.push_str("; 名称,チューニング空間,チャンネル,リモコン番号,サービスタイプ,サービスID,ネットワークID,TSID,状態\r\n");

        for (i, space) in self.spaces.iter().enumerate() {
            if space.channel_list.num_channels() == 0 {
                continue;
            }
            if !space.name.is_empty() {
                buf.push_str(&format!(";#SPACE({},{})\r\n", i, space.name));
            }
            for ch in &space.channel_list.channels {
                let name = csv_quote(&ch.name);
                buf.push_str(&format!(
                    "{},{},{},{},",
                    name, ch.space, ch.channel_index, ch.channel_no
                ));
                if ch.service_type != 0 {
                    buf.push_str(&ch.service_type.to_string());
                }
                buf.push(',');
                if ch.service_id != 0 {
                    buf.push_str(&ch.service_id.to_string());
                }
                buf.push(',');
                if ch.network_id != 0 {
                    buf.push_str(&ch.network_id.to_string());
                }
                buf.push(',');
                if ch.transport_stream_id != 0 {
                    buf.push_str(&ch.transport_stream_id.to_string());
                }
                buf.push(',');
                buf.push(if ch.enabled { '1' } else { '0' });
                buf.push_str("\r\n");
            }
        }
        buf
    }
}

/// CSV フィールドのクォート処理。原実装 SaveToFile:826-851。
fn csv_quote(name: &str) -> String {
    let needs_quote = name.starts_with('#')
        || name.starts_with(';')
        || name.contains(',')
        || name.contains('"');
    if !needs_quote {
        return name.to_string();
    }
    let mut out = String::new();
    out.push('"');
    for c in name.chars() {
        if c == '"' {
            out.push_str("\"\"");
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

/// 1 行の CSV テキストを ChannelInfo へパースする。原実装 LoadFromFile:1090-1163。
fn parse_channel_line(line: &str) -> Option<ChannelInfo> {
    let mut ch = ChannelInfo::default();

    let mut rest = line;

    // チャンネル名(クォート対応)。
    let name_end;
    if rest.starts_with('"') {
        rest = &rest[1..];
        let mut name = String::new();
        loop {
            if let Some(q) = rest.find('"') {
                name.push_str(&rest[..q]);
                rest = &rest[q + 1..];
                if rest.starts_with('"') {
                    name.push('"');
                    rest = &rest[1..];
                } else {
                    break;
                }
            } else {
                name.push_str(rest);
                rest = "";
                break;
            }
        }
        ch.name = name;
        rest = rest.trim_start_matches([' ', '\t']);
        name_end = true;
    } else {
        if let Some(comma) = rest.find(',') {
            ch.name = rest[..comma].to_string();
            rest = &rest[comma..];
        } else {
            return None;
        }
        name_end = true;
    }
    let _ = name_end;

    // 次フィールド: チューニング空間。
    rest = next_token(rest)?;
    if !rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let (space, r) = parse_digits(rest);
    ch.space = space;
    rest = r;

    // 次フィールド: チャンネルインデックス。
    rest = next_token(rest)?;
    if !rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let (idx, r) = parse_digits(rest);
    ch.channel_index = idx;
    rest = r;

    // 以降はオプション。
    if let Some(r) = next_token(rest) {
        let (v, r2) = parse_digits(r);
        ch.channel_no = v;
        rest = r2;

        if let Some(r) = next_token(rest) {
            let (v, r2) = parse_digits(r);
            ch.service_type = v as u8;
            rest = r2;

            if let Some(r) = next_token(rest) {
                let (v, r2) = parse_digits(r);
                ch.service_id = v as u16;
                rest = r2;

                if let Some(r) = next_token(rest) {
                    let (v, r2) = parse_digits(r);
                    ch.network_id = v as u16;
                    rest = r2;

                    if let Some(r) = next_token(rest) {
                        let (v, r2) = parse_digits(r);
                        ch.transport_stream_id = v as u16;
                        rest = r2;

                        if let Some(r) = next_token(rest) {
                            if r.starts_with(|c: char| c.is_ascii_digit()) {
                                let (v, _) = parse_digits(r);
                                ch.enabled = (v & 1) != 0;
                            }
                        }
                    }
                }
            }
        }
    } else {
        ch.enabled = true;
    }

    Some(ch)
}

/// 原実装 SkipSpaces+NextToken:945-963。カンマをスキップしてポインタを進める。
fn next_token(s: &str) -> Option<&str> {
    let s = s.trim_start_matches([' ', '\t']);
    let s = s.strip_prefix(',')?;
    Some(s.trim_start_matches([' ', '\t']))
}

/// 原実装 ParseDigits:970-976。先頭の十進数字列を整数としてパースし、残りを返す。
fn parse_digits(s: &str) -> (i32, &str) {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    let num: i32 = s[..end].parse().unwrap_or(0);
    (num, &s[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ChannelInfo ----

    #[test]
    fn test_channel_info_new() {
        let ch = ChannelInfo::new(0, 5, 3, "NHK総合");
        assert_eq!(ch.space, 0);
        assert_eq!(ch.channel_index, 5);
        assert_eq!(ch.channel_no, 3);
        assert_eq!(ch.name, "NHK総合");
        assert!(ch.enabled);
    }

    #[test]
    fn test_set_channel_no_negative_fails() {
        let mut ch = ChannelInfo::default();
        assert!(!ch.set_channel_no(-1));
        assert_eq!(ch.channel_no, 0);
    }

    // ---- ChannelList: CRUD ----

    #[test]
    fn test_channel_list_add_delete() {
        let mut list = ChannelList::new();
        list.add_channel(ChannelInfo::new(0, 0, 1, "A"));
        list.add_channel(ChannelInfo::new(0, 1, 2, "B"));
        assert_eq!(list.num_channels(), 2);
        list.delete_channel(0);
        assert_eq!(list.num_channels(), 1);
        assert_eq!(list.get_channel(0).unwrap().name, "B");
    }

    #[test]
    fn test_channel_list_insert() {
        let mut list = ChannelList::new();
        list.add_channel(ChannelInfo::new(0, 0, 1, "A"));
        list.add_channel(ChannelInfo::new(0, 2, 3, "C"));
        list.insert_channel(1, ChannelInfo::new(0, 1, 2, "B"));
        assert_eq!(list.num_channels(), 3);
        assert_eq!(list.get_channel(1).unwrap().name, "B");
    }

    // ---- ChannelList: 検索 ----

    #[test]
    fn test_find_by_ids() {
        let mut list = ChannelList::new();
        let mut ch = ChannelInfo::new(0, 0, 1, "CH1");
        ch.network_id = 4;
        ch.transport_stream_id = 101;
        ch.service_id = 1024;
        list.add_channel(ch);
        assert_eq!(list.find_by_ids(4, 101, 1024, false), Some(0));
        assert_eq!(list.find_by_ids(4, 101, 9999, false), None);
    }

    #[test]
    fn test_find_channel_no_enabled_only() {
        let mut list = ChannelList::new();
        let mut ch = ChannelInfo::new(0, 0, 5, "X");
        ch.enabled = false;
        list.add_channel(ch);
        assert_eq!(list.find_channel_no(5, true), None);
        assert_eq!(list.find_channel_no(5, false), Some(0));
    }

    // ---- ChannelList: ナビゲーション ----

    #[test]
    fn test_get_next_channel_basic() {
        let mut list = ChannelList::new();
        list.add_channel(ChannelInfo::new(0, 0, 1, "A"));
        list.add_channel(ChannelInfo::new(0, 1, 2, "B"));
        list.add_channel(ChannelInfo::new(0, 2, 3, "C"));
        assert_eq!(list.get_next_channel(0, false), Some(1));
        assert_eq!(list.get_next_channel(2, false), None);
        assert_eq!(list.get_next_channel(2, true), Some(0));
    }

    #[test]
    fn test_get_prev_channel_basic() {
        let mut list = ChannelList::new();
        list.add_channel(ChannelInfo::new(0, 0, 1, "A"));
        list.add_channel(ChannelInfo::new(0, 1, 2, "B"));
        list.add_channel(ChannelInfo::new(0, 2, 3, "C"));
        assert_eq!(list.get_prev_channel(2, false), Some(1));
        assert_eq!(list.get_prev_channel(0, false), None);
        assert_eq!(list.get_prev_channel(0, true), Some(2));
    }

    // ---- ChannelList: ソート ----

    #[test]
    fn test_sort_by_channel_no() {
        let mut list = ChannelList::new();
        list.add_channel(ChannelInfo::new(0, 0, 3, "C"));
        list.add_channel(ChannelInfo::new(0, 1, 1, "A"));
        list.add_channel(ChannelInfo::new(0, 2, 2, "B"));
        list.sort(SortType::ChannelNo, false);
        assert_eq!(list.get_channel(0).unwrap().channel_no, 1);
        assert_eq!(list.get_channel(2).unwrap().channel_no, 3);
    }

    #[test]
    fn test_sort_by_name_case_insensitive() {
        let mut list = ChannelList::new();
        list.add_channel(ChannelInfo::new(0, 0, 1, "bbb"));
        list.add_channel(ChannelInfo::new(0, 1, 2, "AAA"));
        list.add_channel(ChannelInfo::new(0, 2, 3, "ccc"));
        list.sort(SortType::Name, false);
        assert_eq!(list.get_channel(0).unwrap().name, "AAA");
        assert_eq!(list.get_channel(2).unwrap().name, "ccc");
    }

    // ---- TuningSpaceInfo: set_name ----

    #[test]
    fn test_tuning_space_type_detection() {
        let mut ts = TuningSpaceInfo::new();
        ts.set_name("地上波");
        assert_eq!(ts.space_type, TuningSpaceType::Terrestrial);
        ts.set_name("BS放送");
        assert_eq!(ts.space_type, TuningSpaceType::Bs);
        ts.set_name("CS110");
        assert_eq!(ts.space_type, TuningSpaceType::Cs110);
        ts.set_name("その他");
        assert_eq!(ts.space_type, TuningSpaceType::Unknown);
    }

    // ---- TuningSpaceList: CSV パース ----

    #[test]
    fn test_parse_csv_basic() {
        let csv = "; comment\r\n\
                   ##SPACE(0,地上)\r\n\
                   NHK総合,0,0,1,1,1024,4,101,1\r\n\
                   NHK教育,0,1,2,1,1408,4,101,1\r\n";
        let mut tsl = TuningSpaceList::new();
        tsl.parse_csv(csv);
        assert_eq!(tsl.all_channels.num_channels(), 2);
        assert_eq!(tsl.all_channels.get_channel(0).unwrap().name, "NHK総合");
        assert_eq!(tsl.all_channels.get_channel(0).unwrap().channel_no, 1);
        assert_eq!(tsl.all_channels.get_channel(0).unwrap().service_id, 1024);
        assert_eq!(tsl.spaces[0].name, "地上");
    }

    #[test]
    fn test_parse_csv_quoted_name() {
        let csv = "\"Chan,nel\",0,5,3,,1000,0,0,1\r\n";
        let mut tsl = TuningSpaceList::new();
        tsl.parse_csv(csv);
        assert_eq!(tsl.all_channels.num_channels(), 1);
        assert_eq!(tsl.all_channels.get_channel(0).unwrap().name, "Chan,nel");
    }

    #[test]
    fn test_parse_csv_disabled_channel() {
        let csv = "TestCH,1,3,7,,,,,0\r\n";
        let mut tsl = TuningSpaceList::new();
        tsl.parse_csv(csv);
        assert!(!tsl.all_channels.get_channel(0).unwrap().enabled);
    }

    // ---- TuningSpaceList: CSV シリアライズ ----

    #[test]
    fn test_serialize_csv_basic() {
        let mut tsl = TuningSpaceList::new();
        tsl.reserve(1);
        tsl.spaces[0].set_name("地上");
        let mut ch = ChannelInfo::new(0, 0, 1, "NHK");
        ch.service_id = 1024;
        ch.network_id = 4;
        ch.transport_stream_id = 101;
        tsl.spaces[0].channel_list.add_channel(ch);

        let csv = tsl.serialize_csv();
        assert!(csv.contains(";#SPACE(0,地上)"));
        assert!(csv.contains("NHK,0,0,1,,1024,4,101,1"));
    }

    #[test]
    fn test_csv_roundtrip() {
        let csv_in = "\
; TVTest チャンネル設定ファイル\r\n\
; 名称,チューニング空間,チャンネル,リモコン番号,サービスタイプ,サービスID,ネットワークID,TSID,状態\r\n\
;#SPACE(0,地上)\r\n\
NHK総合,0,0,1,,1024,4,101,1\r\n\
";
        let mut tsl = TuningSpaceList::new();
        tsl.parse_csv(csv_in);
        let csv_out = tsl.serialize_csv();
        assert!(csv_out.contains("NHK総合,0,0,1,,1024,4,101,1"));
        assert!(csv_out.contains(";#SPACE(0,地上)"));
    }

    // ---- has_multi_service / has_remote_control_key_id ----

    #[test]
    fn test_has_multi_service() {
        let mut list = ChannelList::new();
        let mut a = ChannelInfo::new(0, 0, 1, "A");
        a.network_id = 4;
        a.transport_stream_id = 101;
        a.service_id = 100;
        let mut b = ChannelInfo::new(0, 1, 2, "B");
        b.network_id = 4;
        b.transport_stream_id = 101;
        b.service_id = 200;
        list.add_channel(a);
        list.add_channel(b);
        assert!(list.has_multi_service());
    }

    #[test]
    fn test_has_remote_control_key_id() {
        let mut list = ChannelList::new();
        list.add_channel(ChannelInfo::new(0, 0, 0, "X"));
        assert!(!list.has_remote_control_key_id());
        list.get_channel_mut(0).unwrap().channel_no = 3;
        assert!(list.has_remote_control_key_id());
    }
}
