// Rust port of LibISDB/MediaParsers/H264Parser.cpp + H265Parser.cpp

use libisdb_bitstream::BitstreamReader;
use libisdb_mpeg2_video_parser::ebsp_to_rbsp;

// ──────────────────────────────────────────────────────────────
// 共通ユーティリティ: NAL ユニット境界スキャン
// ──────────────────────────────────────────────────────────────

/// アクセスユニット内から次の 3 バイトスタートコード `0x000001` を探す。
/// `start` から検索開始、発見した場合は次のバイト位置(NAL ヘッダの直後)を返す。
/// H264Parser.cpp:55-63 / H265Parser.cpp:56-63
fn find_next_start_code(data: &[u8], start: usize, end: usize) -> Option<usize> {
    let mut sync: u32 = 0xFFFF_FFFF;
    let mut i = start;
    while i < end {
        sync = (sync << 8) | data[i] as u32;
        i += 1;
        if (sync & 0x00FF_FFFF) == 0x0000_0001 {
            return Some(i);
        }
    }
    None
}

// ──────────────────────────────────────────────────────────────
// H.264
// ──────────────────────────────────────────────────────────────

// H264Parser.hpp
#[derive(Debug, Clone, Default, PartialEq)]
pub struct H264Vui {
    pub aspect_ratio_info_present_flag: bool,
    pub aspect_ratio_idc: u8,
    pub sar_width: u16,
    pub sar_height: u16,
    pub overscan_info_present_flag: bool,
    pub overscan_appropriate_flag: bool,
    pub video_signal_type_present_flag: bool,
    pub video_format: u8,
    pub video_full_range_flag: bool,
    pub colour_description_present_flag: bool,
    pub colour_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coefficients: u8,
    pub chroma_loc_info_present_flag: bool,
    pub chroma_sample_loc_type_top_field: u32,
    pub chroma_sample_loc_type_bottom_field: u32,
    pub timing_info_present_flag: bool,
    pub num_units_in_tick: u32,
    pub time_scale: u32,
    pub fixed_frame_rate_flag: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct H264Sps {
    pub profile_idc: u8,
    pub constraint_set0_flag: bool,
    pub constraint_set1_flag: bool,
    pub constraint_set2_flag: bool,
    pub constraint_set3_flag: bool,
    pub level_idc: u8,
    pub seq_parameter_set_id: u32,
    pub chroma_format_idc: u32,
    pub separate_colour_plane_flag: bool,
    pub chroma_array_type: u32,
    pub bit_depth_luma_minus8: u32,
    pub bit_depth_chroma_minus8: u32,
    pub qpprime_y_zero_transform_bypass_flag: bool,
    pub seq_scaling_matrix_present_flag: bool,
    pub log2_max_frame_num_minus4: u32,
    pub pic_order_cnt_type: u32,
    pub log2_max_pic_order_cnt_lsb_minus4: u32,
    pub delta_pic_order_always_zero_flag: bool,
    pub offset_for_non_ref_pic: i32,
    pub offset_for_top_to_bottom_field: i32,
    pub num_ref_frames_in_pic_order_cnt_cycle: u32,
    pub num_ref_frames: u32,
    pub gaps_in_frame_num_value_allowed_flag: bool,
    pub pic_width_in_mbs_minus1: u32,
    pub pic_height_in_map_units_minus1: u32,
    pub frame_mbs_only_flag: bool,
    pub mb_adaptive_frame_field_flag: bool,
    pub direct_8x8_inference_flag: bool,
    pub frame_cropping_flag: bool,
    pub frame_crop_left_offset: u32,
    pub frame_crop_right_offset: u32,
    pub frame_crop_top_offset: u32,
    pub frame_crop_bottom_offset: u32,
    pub vui_parameters_present_flag: bool,
    pub vui: H264Vui,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct H264Aud {
    pub primary_pic_type: u8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct H264Header {
    pub sps: H264Sps,
    pub aud: H264Aud,
}

/// H.264 アクセスユニット。H264Parser.hpp: H264AccessUnit。
#[derive(Debug, Clone, Default)]
pub struct H264AccessUnit {
    data: Vec<u8>,
    pub header: H264Header,
    pub found_sps: bool,
}

impl H264AccessUnit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_data(&self) -> &[u8] {
        &self.data
    }

    pub fn get_size(&self) -> usize {
        self.data.len()
    }

    pub fn clear_size(&mut self) {
        self.data.clear();
    }

    pub fn set_data(&mut self, bytes: &[u8]) {
        self.data.clear();
        self.data.extend_from_slice(bytes);
    }

    pub fn add_data(&mut self, bytes: &[u8]) -> usize {
        self.data.extend_from_slice(bytes);
        self.data.len()
    }

    pub fn trim_tail(&mut self, n: usize) {
        let len = self.data.len();
        self.data.truncate(len.saturating_sub(n));
    }

    pub fn reset(&mut self) {
        self.found_sps = false;
        self.header = H264Header::default();
    }

    // H264Parser.cpp:43-245
    pub fn parse_header(&mut self) -> bool {
        let d = &self.data;
        if d.len() < 5 || d[0] != 0x00 || d[1] != 0x00 || d[2] != 0x01 {
            return false;
        }

        self.found_sps = false;
        self.header = H264Header::default();

        let mut pos = 3usize;
        loop {
            let next_pos = match find_next_start_code(d, pos + 1, d.len()) {
                Some(p) => p,
                None => break,
            };

            let nal_unit_type = d[pos] & 0x1F;
            pos += 1;
            let nal_unit_size_raw = next_pos.saturating_sub(3).saturating_sub(pos);

            let mut nal_buf: Vec<u8> = d[pos..pos + nal_unit_size_raw].to_vec();
            let nal_unit_size = match ebsp_to_rbsp(&mut nal_buf) {
                Some(s) => s,
                None => break,
            };

            if nal_unit_type == 0x07 {
                // Sequence Parameter Set
                let mut bs = BitstreamReader::new(&nal_buf[..nal_unit_size]);
                let sps = &mut self.header.sps;

                sps.profile_idc = bs.get_bits(8) as u8;
                sps.constraint_set0_flag = bs.get_flag();
                sps.constraint_set1_flag = bs.get_flag();
                sps.constraint_set2_flag = bs.get_flag();
                sps.constraint_set3_flag = bs.get_flag();
                if bs.get_bits(4) != 0 {
                    return false; // reserved_zero_4bits
                }
                sps.level_idc = bs.get_bits(8) as u8;
                sps.seq_parameter_set_id = bs.get_ue_v().unwrap_or(0);
                sps.chroma_format_idc = 1;
                sps.separate_colour_plane_flag = false;
                sps.bit_depth_luma_minus8 = 0;
                sps.bit_depth_chroma_minus8 = 0;
                sps.qpprime_y_zero_transform_bypass_flag = false;
                sps.seq_scaling_matrix_present_flag = false;

                if matches!(sps.profile_idc, 100 | 110 | 122 | 244 | 44 | 83 | 86) {
                    sps.chroma_format_idc = bs.get_ue_v().unwrap_or(0);
                    if sps.chroma_format_idc == 3 {
                        sps.separate_colour_plane_flag = bs.get_flag();
                    }
                    sps.bit_depth_luma_minus8 = bs.get_ue_v().unwrap_or(0);
                    sps.bit_depth_chroma_minus8 = bs.get_ue_v().unwrap_or(0);
                    sps.qpprime_y_zero_transform_bypass_flag = bs.get_flag();
                    sps.seq_scaling_matrix_present_flag = bs.get_flag();
                    if sps.seq_scaling_matrix_present_flag {
                        let length = if sps.chroma_format_idc != 3 { 8 } else { 12 };
                        for i in 0..length {
                            if bs.get_flag() {
                                let mut last_scale = 8i32;
                                let mut next_scale = 8i32;
                                let size = if i < 6 { 16 } else { 64 };
                                for _ in 0..size {
                                    if next_scale != 0 {
                                        let delta = bs.get_se_v().unwrap_or(0);
                                        next_scale = (last_scale + delta + 256) % 256;
                                        last_scale = next_scale;
                                    }
                                }
                            }
                        }
                    }
                }

                sps.log2_max_frame_num_minus4 = bs.get_ue_v().unwrap_or(0);
                sps.pic_order_cnt_type = bs.get_ue_v().unwrap_or(0);
                if sps.pic_order_cnt_type == 0 {
                    sps.log2_max_pic_order_cnt_lsb_minus4 = bs.get_ue_v().unwrap_or(0);
                } else if sps.pic_order_cnt_type == 1 {
                    sps.delta_pic_order_always_zero_flag = bs.get_flag();
                    sps.offset_for_non_ref_pic = bs.get_se_v().unwrap_or(0);
                    sps.offset_for_top_to_bottom_field = bs.get_se_v().unwrap_or(0);
                    sps.num_ref_frames_in_pic_order_cnt_cycle = bs.get_ue_v().unwrap_or(0);
                    for _ in 0..sps.num_ref_frames_in_pic_order_cnt_cycle {
                        bs.get_se_v().unwrap_or(0);
                    }
                }

                sps.num_ref_frames = bs.get_ue_v().unwrap_or(0);
                sps.gaps_in_frame_num_value_allowed_flag = bs.get_flag();
                sps.pic_width_in_mbs_minus1 = bs.get_ue_v().unwrap_or(0);
                sps.pic_height_in_map_units_minus1 = bs.get_ue_v().unwrap_or(0);
                sps.frame_mbs_only_flag = bs.get_flag();
                if !sps.frame_mbs_only_flag {
                    sps.mb_adaptive_frame_field_flag = bs.get_flag();
                }
                sps.direct_8x8_inference_flag = bs.get_flag();
                sps.frame_cropping_flag = bs.get_flag();
                if sps.frame_cropping_flag {
                    sps.frame_crop_left_offset = bs.get_ue_v().unwrap_or(0);
                    sps.frame_crop_right_offset = bs.get_ue_v().unwrap_or(0);
                    sps.frame_crop_top_offset = bs.get_ue_v().unwrap_or(0);
                    sps.frame_crop_bottom_offset = bs.get_ue_v().unwrap_or(0);
                }
                sps.vui_parameters_present_flag = bs.get_flag();
                if sps.vui_parameters_present_flag {
                    let vui = &mut sps.vui;
                    vui.aspect_ratio_info_present_flag = bs.get_flag();
                    if vui.aspect_ratio_info_present_flag {
                        vui.aspect_ratio_idc = bs.get_bits(8) as u8;
                        if vui.aspect_ratio_idc == 255 {
                            vui.sar_width = bs.get_bits(16) as u16;
                            vui.sar_height = bs.get_bits(16) as u16;
                        }
                    }
                    vui.overscan_info_present_flag = bs.get_flag();
                    if vui.overscan_info_present_flag {
                        vui.overscan_appropriate_flag = bs.get_flag();
                    }
                    vui.video_signal_type_present_flag = bs.get_flag();
                    if vui.video_signal_type_present_flag {
                        vui.video_format = bs.get_bits(3) as u8;
                        vui.video_full_range_flag = bs.get_flag();
                        vui.colour_description_present_flag = bs.get_flag();
                        if vui.colour_description_present_flag {
                            vui.colour_primaries = bs.get_bits(8) as u8;
                            vui.transfer_characteristics = bs.get_bits(8) as u8;
                            vui.matrix_coefficients = bs.get_bits(8) as u8;
                        }
                    }
                    vui.chroma_loc_info_present_flag = bs.get_flag();
                    if vui.chroma_loc_info_present_flag {
                        vui.chroma_sample_loc_type_top_field = bs.get_ue_v().unwrap_or(0);
                        vui.chroma_sample_loc_type_bottom_field = bs.get_ue_v().unwrap_or(0);
                    }
                    vui.timing_info_present_flag = bs.get_flag();
                    if vui.timing_info_present_flag {
                        vui.num_units_in_tick = bs.get_bits(32);
                        vui.time_scale = bs.get_bits(32);
                        vui.fixed_frame_rate_flag = bs.get_flag();
                    }
                }

                sps.chroma_array_type = if sps.separate_colour_plane_flag {
                    0
                } else {
                    sps.chroma_format_idc
                };

                self.found_sps = true;
            } else if nal_unit_type == 0x09 {
                // Access unit delimiter
                self.header.aud.primary_pic_type = d[pos] >> 5;
            } else if nal_unit_type == 0x0A {
                // End of sequence
                break;
            }

            pos = next_pos;
        }

        self.found_sps
    }

    // H264Parser.cpp:255-266
    pub fn get_horizontal_size(&self) -> u16 {
        let sps = &self.header.sps;
        let mut width = (sps.pic_width_in_mbs_minus1 + 1) * 16;
        if sps.frame_cropping_flag {
            let mut crop = sps.frame_crop_left_offset + sps.frame_crop_right_offset;
            if sps.chroma_array_type != 0 {
                crop *= self.get_sub_width_c() as u32;
            }
            if crop < width {
                width -= crop;
            }
        }
        width as u16
    }

    // H264Parser.cpp:269-284
    pub fn get_vertical_size(&self) -> u16 {
        let sps = &self.header.sps;
        let mut height = (sps.pic_height_in_map_units_minus1 + 1) * 16;
        if !sps.frame_mbs_only_flag {
            height *= 2;
        }
        if sps.frame_cropping_flag {
            let mut crop = sps.frame_crop_top_offset + sps.frame_crop_bottom_offset;
            if sps.chroma_array_type != 0 {
                crop *= self.get_sub_height_c() as u32;
            }
            if !sps.frame_mbs_only_flag {
                crop *= 2;
            }
            if crop < height {
                height -= crop;
            }
        }
        height as u16
    }

    // H264Parser.cpp:287-330
    pub fn get_sar(&self) -> Option<(u16, u16)> {
        const SAR_LIST: [(u8, u8); 17] = [
            (0, 0), (1, 1), (12, 11), (10, 11), (16, 11), (40, 33),
            (24, 11), (20, 11), (32, 11), (80, 33), (18, 11), (15, 11),
            (64, 33), (160, 99), (4, 3), (3, 2), (2, 1),
        ];
        let sps = &self.header.sps;
        if !sps.vui_parameters_present_flag || !sps.vui.aspect_ratio_info_present_flag {
            return None;
        }
        let idc = sps.vui.aspect_ratio_idc;
        if (idc as usize) < SAR_LIST.len() {
            let (h, v) = SAR_LIST[idc as usize];
            Some((h as u16, v as u16))
        } else if idc == 255 {
            Some((sps.vui.sar_width, sps.vui.sar_height))
        } else {
            None
        }
    }

    // H264Parser.cpp:333-346
    pub fn get_timing_info(&self) -> Option<(u32, u32, bool)> {
        let sps = &self.header.sps;
        if !sps.vui_parameters_present_flag || !sps.vui.timing_info_present_flag {
            return None;
        }
        Some((sps.vui.num_units_in_tick, sps.vui.time_scale, sps.vui.fixed_frame_rate_flag))
    }

    fn get_sub_width_c(&self) -> i32 {
        let cfi = self.header.sps.chroma_format_idc;
        if cfi == 1 || cfi == 2 { 2 } else { 1 }
    }

    fn get_sub_height_c(&self) -> i32 {
        if self.header.sps.chroma_format_idc == 1 { 2 } else { 1 }
    }
}

/// H.264 パーサー。MPEGVideoParserBase + H264Parser。
pub struct H264Parser {
    sync_state: u32,
    access_unit: H264AccessUnit,
}

impl H264Parser {
    pub fn new() -> Self {
        Self {
            sync_state: 0xFFFF_FFFF,
            access_unit: H264AccessUnit::new(),
        }
    }

    pub fn reset(&mut self) {
        self.sync_state = 0xFFFF_FFFF;
        self.access_unit.reset();
        self.access_unit.clear_size();
    }

    // H264Parser.cpp:370-373: StoreES (start_code=0x00000109, mask=0xFFFFFF1F)
    pub fn store_es<F>(&mut self, data: &[u8], handler: &mut F) -> bool
    where
        F: FnMut(&H264AccessUnit),
    {
        self.parse_sequence(data, 0x0000_0109, 0xFFFF_FF1F, handler)
    }

    // MPEGVideoParser.cpp:63-131: ParseSequence (マスク版)
    fn parse_sequence<F>(
        &mut self,
        data: &[u8],
        start_code: u32,
        start_code_mask: u32,
        handler: &mut F,
    ) -> bool
    where
        F: FnMut(&H264AccessUnit),
    {
        let mut found = false;
        let mut sync_state = self.sync_state;
        let size = data.len();
        let mut pos = 0usize;

        while pos < size {
            let remain = size - pos;
            let mut start = 0usize;

            while start < remain {
                sync_state = (sync_state << 8) | data[pos + start] as u32;
                start += 1;
                if (sync_state & start_code_mask) == start_code {
                    break;
                }
            }

            if start < remain {
                if self.access_unit.get_size() >= 4 {
                    if start > 4 {
                        let chunk = &data[pos..pos + start - 4];
                        self.access_unit.add_data(chunk);
                    } else if start < 4 {
                        self.access_unit.trim_tail(4 - start);
                    }

                    let mut au = std::mem::take(&mut self.access_unit);
                    if au.parse_header() {
                        handler(&au);
                    }
                    self.access_unit = au;
                    self.access_unit.clear_size();
                    found = true;
                }

                let sc_bytes = [
                    (sync_state >> 24) as u8,
                    (sync_state >> 16) as u8,
                    (sync_state >> 8) as u8,
                    sync_state as u8,
                ];
                self.access_unit.set_data(&sc_bytes);
                sync_state = 0xFFFF_FFFF;
                pos += start;
            } else {
                if self.access_unit.get_size() >= 4 {
                    let added = self.access_unit.add_data(&data[pos..]);
                    if added >= 0x100_0000 {
                        self.access_unit.clear_size();
                    }
                }
                break;
            }
        }

        self.sync_state = sync_state;
        found
    }
}

impl Default for H264Parser {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────────────────────
// H.265
// ──────────────────────────────────────────────────────────────

const H265_MAX_SUB_LAYERS: usize = 7;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct H265SubLayerPtl {
    pub sub_layer_profile_present_flag: bool,
    pub sub_layer_level_present_flag: bool,
    pub sub_layer_profile_space: u8,
    pub sub_layer_tier_flag: bool,
    pub sub_layer_profile_idc: u8,
    pub sub_layer_profile_compatibility_flag: [bool; 32],
    pub sub_layer_progressive_source_flag: bool,
    pub sub_layer_interlaced_source_flag: bool,
    pub sub_layer_non_packed_constraint_flag: bool,
    pub sub_layer_frame_only_constraint_flag: bool,
    pub sub_layer_level_idc: u8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct H265Ptl {
    pub general_profile_space: u8,
    pub general_tier_flag: bool,
    pub general_profile_idc: u8,
    pub general_profile_compatibility_flag: [bool; 32],
    pub general_progressive_source_flag: bool,
    pub general_interlaced_source_flag: bool,
    pub general_non_packed_constraint_flag: bool,
    pub general_frame_only_constraint_flag: bool,
    pub general_level_idc: u8,
    pub sub_layer: [H265SubLayerPtl; H265_MAX_SUB_LAYERS],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct H265SubLayerOrderingInfo {
    pub sps_max_dec_pic_buffering_minus1: u32,
    pub sps_max_num_reorder_pics: u32,
    pub sps_max_latency_increase_plus1: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct H265Vui {
    pub aspect_ratio_info_present_flag: bool,
    pub aspect_ratio_idc: u8,
    pub sar_width: u16,
    pub sar_height: u16,
    pub overscan_info_present_flag: bool,
    pub overscan_appropriate_flag: bool,
    pub video_signal_type_present_flag: bool,
    pub video_format: u8,
    pub video_full_range_flag: bool,
    pub colour_description_present_flag: bool,
    pub colour_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coeffs: u8,
    pub chroma_loc_info_present_flag: bool,
    pub chroma_sample_loc_type_top_field: u32,
    pub chroma_sample_loc_type_bottom_field: u32,
    pub neutral_chroma_indication_flag: bool,
    pub field_seq_flag: bool,
    pub frame_field_info_present_flag: bool,
    pub default_display_window_flag: bool,
    pub def_disp_win_left_offset: u32,
    pub def_disp_win_right_offset: u32,
    pub def_disp_win_top_offset: u32,
    pub def_disp_win_bottom_offset: u32,
    pub vui_timing_info_present_flag: bool,
    pub vui_num_units_in_tick: u32,
    pub vui_time_scale: u32,
    pub vui_poc_proportional_to_timing_flag: bool,
    pub vui_num_ticks_poc_diff_one_minus1: u32,
    pub vui_hrd_parameters_present_flag: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct H265Sps {
    pub sps_video_parameter_set_id: u8,
    pub sps_max_sub_layers_minus1: u8,
    pub sps_temporal_id_nesting_flag: bool,
    pub ptl: H265Ptl,
    pub sps_seq_parameter_set_id: u32,
    pub chroma_format_idc: u32,
    pub separate_colour_plane_flag: bool,
    pub pic_width_in_luma_samples: u32,
    pub pic_height_in_luma_samples: u32,
    pub conformance_window_flag: bool,
    pub conf_win_left_offset: u32,
    pub conf_win_right_offset: u32,
    pub conf_win_top_offset: u32,
    pub conf_win_bottom_offset: u32,
    pub bit_depth_luma_minus8: u32,
    pub bit_depth_chroma_minus8: u32,
    pub log2_max_pic_order_cnt_lsb_minus4: u32,
    pub sps_sub_layer_ordering_info_present_flag: bool,
    pub sub_layer_ordering_info: [H265SubLayerOrderingInfo; H265_MAX_SUB_LAYERS],
    pub log2_min_luma_coding_block_size_minus3: u32,
    pub log2_diff_max_min_luma_coding_block_size: u32,
    pub log2_min_transform_block_size_minus2: u32,
    pub log2_diff_max_min_transform_block_size: u32,
    pub max_transform_hierarchy_depth_inter: u32,
    pub max_transform_hierarchy_depth_intra: u32,
    pub scaling_list_enabled_flag: bool,
    pub sps_scaling_list_data_present_flag: bool,
    pub amp_enabled_flag: bool,
    pub sample_adaptive_offset_enabled_flag: bool,
    pub pcm_enabled_flag: bool,
    pub pcm_sample_bit_depth_luma_minus1: u8,
    pub pcm_sample_bit_depth_chroma_minus1: u8,
    pub log2_min_pcm_luma_coding_block_size_minus3: u32,
    pub log2_diff_max_min_pcm_luma_coding_block_size: u32,
    pub pcm_loop_filter_disabled_flag: bool,
    pub num_short_term_ref_pic_sets: u32,
    pub long_term_ref_pics_present_flag: bool,
    pub num_long_term_ref_pics_sps: u32,
    pub sps_temporal_mvp_enabled_flag: bool,
    pub strong_intra_smoothing_enabled_flag: bool,
    pub vui_parameters_present_flag: bool,
    pub vui: H265Vui,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct H265Aud {
    pub pic_type: u8,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct H265Header {
    pub sps: H265Sps,
    pub aud: H265Aud,
}

/// H.265 アクセスユニット。H265Parser.hpp: H265AccessUnit。
#[derive(Debug, Clone, Default)]
pub struct H265AccessUnit {
    data: Vec<u8>,
    pub header: H265Header,
    pub found_sps: bool,
}

impl H265AccessUnit {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_data(&self) -> &[u8] {
        &self.data
    }

    pub fn get_size(&self) -> usize {
        self.data.len()
    }

    pub fn clear_size(&mut self) {
        self.data.clear();
    }

    pub fn set_data(&mut self, bytes: &[u8]) {
        self.data.clear();
        self.data.extend_from_slice(bytes);
    }

    pub fn add_data(&mut self, bytes: &[u8]) -> usize {
        self.data.extend_from_slice(bytes);
        self.data.len()
    }

    pub fn trim_tail(&mut self, n: usize) {
        let len = self.data.len();
        self.data.truncate(len.saturating_sub(n));
    }

    pub fn reset(&mut self) {
        self.found_sps = false;
        self.header = H265Header::default();
    }

    // H265Parser.cpp:44-313
    pub fn parse_header(&mut self) -> bool {
        let d = &self.data;
        if d.len() < 6 || d[0] != 0x00 || d[1] != 0x00 || d[2] != 0x01 {
            return false;
        }

        self.found_sps = false;
        self.header = H265Header::default();

        let mut pos = 3usize;
        loop {
            let next_pos = match find_next_start_code(d, pos + 1, d.len()) {
                Some(p) => p,
                None => break,
            };

            if d[pos] & 0x80 != 0 {
                break; // forbidden_zero_bit
            }
            let nal_unit_type = (d[pos] & 0x7E) >> 1;
            pos += 2; // skip nal_unit_header (2 bytes)
            let nal_unit_size_raw = next_pos.saturating_sub(3).saturating_sub(pos);

            let mut nal_buf: Vec<u8> = d[pos..pos + nal_unit_size_raw].to_vec();
            let nal_unit_size = match ebsp_to_rbsp(&mut nal_buf) {
                Some(s) => s,
                None => break,
            };

            if nal_unit_type == 0x21 {
                // Sequence Parameter Set
                let mut bs = BitstreamReader::new(&nal_buf[..nal_unit_size]);
                let sps = &mut self.header.sps;

                sps.sps_video_parameter_set_id = bs.get_bits(4) as u8;
                sps.sps_max_sub_layers_minus1 = bs.get_bits(3) as u8;
                sps.sps_temporal_id_nesting_flag = bs.get_flag();

                // profile_tier_level
                let ptl = &mut sps.ptl;
                ptl.general_profile_space = bs.get_bits(2) as u8;
                ptl.general_tier_flag = bs.get_flag();
                ptl.general_profile_idc = bs.get_bits(5) as u8;
                for i in 0..32 {
                    ptl.general_profile_compatibility_flag[i] = bs.get_flag();
                }
                ptl.general_progressive_source_flag = bs.get_flag();
                ptl.general_interlaced_source_flag = bs.get_flag();
                ptl.general_non_packed_constraint_flag = bs.get_flag();
                ptl.general_frame_only_constraint_flag = bs.get_flag();
                bs.skip(44);
                ptl.general_level_idc = bs.get_bits(8) as u8;

                let max_sub = sps.sps_max_sub_layers_minus1 as usize;
                for i in 0..max_sub {
                    ptl.sub_layer[i].sub_layer_profile_present_flag = bs.get_flag();
                    ptl.sub_layer[i].sub_layer_level_present_flag = bs.get_flag();
                }
                if max_sub > 0 {
                    bs.skip((8 - max_sub) * 2);
                }
                for i in 0..max_sub {
                    if ptl.sub_layer[i].sub_layer_profile_present_flag {
                        ptl.sub_layer[i].sub_layer_profile_space = bs.get_bits(2) as u8;
                        ptl.sub_layer[i].sub_layer_tier_flag = bs.get_flag();
                        ptl.sub_layer[i].sub_layer_profile_idc = bs.get_bits(5) as u8;
                        for j in 0..32 {
                            ptl.sub_layer[i].sub_layer_profile_compatibility_flag[j] = bs.get_flag();
                        }
                        ptl.sub_layer[i].sub_layer_progressive_source_flag = bs.get_flag();
                        ptl.sub_layer[i].sub_layer_interlaced_source_flag = bs.get_flag();
                        ptl.sub_layer[i].sub_layer_non_packed_constraint_flag = bs.get_flag();
                        ptl.sub_layer[i].sub_layer_frame_only_constraint_flag = bs.get_flag();
                        bs.skip(44);
                    }
                    if ptl.sub_layer[i].sub_layer_level_present_flag {
                        ptl.sub_layer[i].sub_layer_level_idc = bs.get_bits(8) as u8;
                    }
                }

                sps.sps_seq_parameter_set_id = bs.get_ue_v().unwrap_or(0);
                sps.chroma_format_idc = bs.get_ue_v().unwrap_or(0);
                if sps.chroma_format_idc == 3 {
                    sps.separate_colour_plane_flag = bs.get_flag();
                }
                sps.pic_width_in_luma_samples = bs.get_ue_v().unwrap_or(0);
                sps.pic_height_in_luma_samples = bs.get_ue_v().unwrap_or(0);
                sps.conformance_window_flag = bs.get_flag();
                if sps.conformance_window_flag {
                    sps.conf_win_left_offset = bs.get_ue_v().unwrap_or(0);
                    sps.conf_win_right_offset = bs.get_ue_v().unwrap_or(0);
                    sps.conf_win_top_offset = bs.get_ue_v().unwrap_or(0);
                    sps.conf_win_bottom_offset = bs.get_ue_v().unwrap_or(0);
                }
                sps.bit_depth_luma_minus8 = bs.get_ue_v().unwrap_or(0);
                sps.bit_depth_chroma_minus8 = bs.get_ue_v().unwrap_or(0);
                sps.log2_max_pic_order_cnt_lsb_minus4 = bs.get_ue_v().unwrap_or(0);
                sps.sps_sub_layer_ordering_info_present_flag = bs.get_flag();
                let start_i = if sps.sps_sub_layer_ordering_info_present_flag {
                    0
                } else {
                    max_sub
                };
                for i in start_i..=max_sub {
                    sps.sub_layer_ordering_info[i].sps_max_dec_pic_buffering_minus1 = bs.get_ue_v().unwrap_or(0);
                    sps.sub_layer_ordering_info[i].sps_max_num_reorder_pics = bs.get_ue_v().unwrap_or(0);
                    sps.sub_layer_ordering_info[i].sps_max_latency_increase_plus1 = bs.get_ue_v().unwrap_or(0);
                }
                sps.log2_min_luma_coding_block_size_minus3 = bs.get_ue_v().unwrap_or(0);
                sps.log2_diff_max_min_luma_coding_block_size = bs.get_ue_v().unwrap_or(0);
                sps.log2_min_transform_block_size_minus2 = bs.get_ue_v().unwrap_or(0);
                sps.log2_diff_max_min_transform_block_size = bs.get_ue_v().unwrap_or(0);
                sps.max_transform_hierarchy_depth_inter = bs.get_ue_v().unwrap_or(0);
                sps.max_transform_hierarchy_depth_intra = bs.get_ue_v().unwrap_or(0);
                sps.scaling_list_enabled_flag = bs.get_flag();
                if sps.scaling_list_enabled_flag {
                    sps.sps_scaling_list_data_present_flag = bs.get_flag();
                    if sps.sps_scaling_list_data_present_flag {
                        for size_id in 0..4i32 {
                            let num_matrix = if size_id == 3 { 2 } else { 6 };
                            for _ in 0..num_matrix {
                                if bs.get_flag() {
                                    // scaling_list_pred_mode_flag = true
                                    let mut next_coef = 8i32;
                                    let coef_num = std::cmp::min(64, 1 << (4 + (size_id * 2)));
                                    if size_id > 1 {
                                        let dc = bs.get_se_v().unwrap_or(0);
                                        next_coef = dc + 8;
                                    }
                                    for _ in 0..coef_num {
                                        let delta = bs.get_se_v().unwrap_or(0);
                                        next_coef = (next_coef + delta + 256) % 256;
                                    }
                                } else {
                                    bs.get_ue_v().unwrap_or(0); // scaling_list_pred_matrix_id_delta
                                }
                            }
                        }
                    }
                }
                sps.amp_enabled_flag = bs.get_flag();
                sps.sample_adaptive_offset_enabled_flag = bs.get_flag();
                sps.pcm_enabled_flag = bs.get_flag();
                if sps.pcm_enabled_flag {
                    sps.pcm_sample_bit_depth_luma_minus1 = bs.get_bits(4) as u8;
                    sps.pcm_sample_bit_depth_chroma_minus1 = bs.get_bits(4) as u8;
                    sps.log2_min_pcm_luma_coding_block_size_minus3 = bs.get_ue_v().unwrap_or(0);
                    sps.log2_diff_max_min_pcm_luma_coding_block_size = bs.get_ue_v().unwrap_or(0);
                    sps.pcm_loop_filter_disabled_flag = bs.get_flag();
                }
                sps.num_short_term_ref_pic_sets = bs.get_ue_v().unwrap_or(0);
                let mut num_pics = 0i32;
                for i in 0..sps.num_short_term_ref_pic_sets as i32 {
                    let inter_ref_pic_set_prediction_flag = if i != 0 { bs.get_flag() } else { false };
                    if inter_ref_pic_set_prediction_flag {
                        bs.get_flag(); // delta_rps_sign
                        bs.get_ue_v().unwrap_or(0); // abs_delta_rps_minus1
                        let mut num_pics_new = 0i32;
                        for _ in 0..=num_pics {
                            let used_by_curr = bs.get_flag();
                            if used_by_curr {
                                num_pics_new += 1;
                            } else if bs.get_flag() {
                                num_pics_new += 1;
                            }
                        }
                        num_pics = num_pics_new;
                    } else {
                        let neg = bs.get_ue_v().unwrap_or(0) as i32;
                        let pos2 = bs.get_ue_v().unwrap_or(0) as i32;
                        num_pics = neg + pos2;
                        for _ in 0..neg {
                            bs.get_ue_v().unwrap_or(0); bs.get_flag();
                        }
                        for _ in 0..pos2 {
                            bs.get_ue_v().unwrap_or(0); bs.get_flag();
                        }
                    }
                }
                sps.long_term_ref_pics_present_flag = bs.get_flag();
                if sps.long_term_ref_pics_present_flag {
                    sps.num_long_term_ref_pics_sps = bs.get_ue_v().unwrap_or(0);
                    for _ in 0..sps.num_long_term_ref_pics_sps {
                        bs.skip((sps.log2_max_pic_order_cnt_lsb_minus4 + 4) as usize);
                        bs.get_flag();
                    }
                }
                sps.sps_temporal_mvp_enabled_flag = bs.get_flag();
                sps.strong_intra_smoothing_enabled_flag = bs.get_flag();
                sps.vui_parameters_present_flag = bs.get_flag();
                if sps.vui_parameters_present_flag {
                    let vui = &mut sps.vui;
                    vui.aspect_ratio_info_present_flag = bs.get_flag();
                    if vui.aspect_ratio_info_present_flag {
                        vui.aspect_ratio_idc = bs.get_bits(8) as u8;
                        if vui.aspect_ratio_idc == 0xFF {
                            vui.sar_width = bs.get_bits(16) as u16;
                            vui.sar_height = bs.get_bits(16) as u16;
                        }
                    }
                    vui.overscan_info_present_flag = bs.get_flag();
                    if vui.overscan_info_present_flag {
                        vui.overscan_appropriate_flag = bs.get_flag();
                    }
                    vui.video_signal_type_present_flag = bs.get_flag();
                    if vui.video_signal_type_present_flag {
                        vui.video_format = bs.get_bits(3) as u8;
                        vui.video_full_range_flag = bs.get_flag();
                        vui.colour_description_present_flag = bs.get_flag();
                        if vui.colour_description_present_flag {
                            vui.colour_primaries = bs.get_bits(8) as u8;
                            vui.transfer_characteristics = bs.get_bits(8) as u8;
                            vui.matrix_coeffs = bs.get_bits(8) as u8;
                        }
                    }
                    vui.chroma_loc_info_present_flag = bs.get_flag();
                    if vui.chroma_loc_info_present_flag {
                        vui.chroma_sample_loc_type_top_field = bs.get_ue_v().unwrap_or(0);
                        vui.chroma_sample_loc_type_bottom_field = bs.get_ue_v().unwrap_or(0);
                    }
                    vui.neutral_chroma_indication_flag = bs.get_flag();
                    vui.field_seq_flag = bs.get_flag();
                    vui.frame_field_info_present_flag = bs.get_flag();
                    vui.default_display_window_flag = bs.get_flag();
                    if vui.default_display_window_flag {
                        vui.def_disp_win_left_offset = bs.get_ue_v().unwrap_or(0);
                        vui.def_disp_win_right_offset = bs.get_ue_v().unwrap_or(0);
                        vui.def_disp_win_top_offset = bs.get_ue_v().unwrap_or(0);
                        vui.def_disp_win_bottom_offset = bs.get_ue_v().unwrap_or(0);
                    }
                    vui.vui_timing_info_present_flag = bs.get_flag();
                    if vui.vui_timing_info_present_flag {
                        vui.vui_num_units_in_tick = bs.get_bits(32);
                        vui.vui_time_scale = bs.get_bits(32);
                        vui.vui_poc_proportional_to_timing_flag = bs.get_flag();
                        if vui.vui_poc_proportional_to_timing_flag {
                            vui.vui_num_ticks_poc_diff_one_minus1 = bs.get_ue_v().unwrap_or(0);
                        }
                        vui.vui_hrd_parameters_present_flag = bs.get_flag();
                    }
                }

                self.found_sps = true;
            } else if nal_unit_type == 0x23 {
                self.header.aud.pic_type = d[pos] >> 5;
            } else if nal_unit_type == 0x24 {
                break;
            }

            pos = next_pos;
        }

        self.found_sps
    }

    // H265Parser.cpp:324-333
    pub fn get_horizontal_size(&self) -> u16 {
        let sps = &self.header.sps;
        let mut w = sps.pic_width_in_luma_samples;
        if sps.conformance_window_flag {
            let crop = (sps.conf_win_left_offset + sps.conf_win_right_offset)
                * self.get_sub_width_c() as u32;
            if crop < w { w -= crop; }
        }
        w as u16
    }

    // H265Parser.cpp:336-345
    pub fn get_vertical_size(&self) -> u16 {
        let sps = &self.header.sps;
        let mut h = sps.pic_height_in_luma_samples;
        if sps.conformance_window_flag {
            let crop = (sps.conf_win_top_offset + sps.conf_win_bottom_offset)
                * self.get_sub_height_c() as u32;
            if crop < h { h -= crop; }
        }
        h as u16
    }

    // H265Parser.cpp:348-391
    pub fn get_sar(&self) -> Option<(u16, u16)> {
        const SAR_LIST: [(u8, u8); 17] = [
            (0, 0), (1, 1), (12, 11), (10, 11), (16, 11), (40, 33),
            (24, 11), (20, 11), (32, 11), (80, 33), (18, 11), (15, 11),
            (64, 33), (160, 99), (4, 3), (3, 2), (2, 1),
        ];
        let sps = &self.header.sps;
        if !sps.vui_parameters_present_flag || !sps.vui.aspect_ratio_info_present_flag {
            return None;
        }
        let idc = sps.vui.aspect_ratio_idc;
        if (idc as usize) < SAR_LIST.len() {
            let (h, v) = SAR_LIST[idc as usize];
            Some((h as u16, v as u16))
        } else if idc == 255 {
            Some((sps.vui.sar_width, sps.vui.sar_height))
        } else {
            None
        }
    }

    // H265Parser.cpp:394-406
    pub fn get_timing_info(&self) -> Option<(u32, u32)> {
        let sps = &self.header.sps;
        if !sps.vui_parameters_present_flag || !sps.vui.vui_timing_info_present_flag {
            return None;
        }
        Some((sps.vui.vui_num_units_in_tick, sps.vui.vui_time_scale))
    }

    fn get_sub_width_c(&self) -> i32 {
        let cfi = self.header.sps.chroma_format_idc;
        if cfi == 1 || cfi == 2 { 2 } else { 1 }
    }

    fn get_sub_height_c(&self) -> i32 {
        if self.header.sps.chroma_format_idc == 1 { 2 } else { 1 }
    }
}

/// H.265 パーサー。
pub struct H265Parser {
    sync_state: u32,
    access_unit: H265AccessUnit,
}

impl H265Parser {
    pub fn new() -> Self {
        Self {
            sync_state: 0xFFFF_FFFF,
            access_unit: H265AccessUnit::new(),
        }
    }

    pub fn reset(&mut self) {
        self.sync_state = 0xFFFF_FFFF;
        self.access_unit.reset();
        self.access_unit.clear_size();
    }

    // H265Parser.cpp:430-433: StoreES (start_code=0x00000146, mask=0xFFFFFFFE)
    pub fn store_es<F>(&mut self, data: &[u8], handler: &mut F) -> bool
    where
        F: FnMut(&H265AccessUnit),
    {
        self.parse_sequence(data, 0x0000_0146, 0xFFFF_FFFE, handler)
    }

    fn parse_sequence<F>(
        &mut self,
        data: &[u8],
        start_code: u32,
        start_code_mask: u32,
        handler: &mut F,
    ) -> bool
    where
        F: FnMut(&H265AccessUnit),
    {
        let mut found = false;
        let mut sync_state = self.sync_state;
        let size = data.len();
        let mut pos = 0usize;

        while pos < size {
            let remain = size - pos;
            let mut start = 0usize;

            while start < remain {
                sync_state = (sync_state << 8) | data[pos + start] as u32;
                start += 1;
                if (sync_state & start_code_mask) == start_code {
                    break;
                }
            }

            if start < remain {
                if self.access_unit.get_size() >= 4 {
                    if start > 4 {
                        self.access_unit.add_data(&data[pos..pos + start - 4]);
                    } else if start < 4 {
                        self.access_unit.trim_tail(4 - start);
                    }

                    let mut au = std::mem::take(&mut self.access_unit);
                    if au.parse_header() {
                        handler(&au);
                    }
                    self.access_unit = au;
                    self.access_unit.clear_size();
                    found = true;
                }

                let sc_bytes = [
                    (sync_state >> 24) as u8,
                    (sync_state >> 16) as u8,
                    (sync_state >> 8) as u8,
                    sync_state as u8,
                ];
                self.access_unit.set_data(&sc_bytes);
                sync_state = 0xFFFF_FFFF;
                pos += start;
            } else {
                if self.access_unit.get_size() >= 4 {
                    let added = self.access_unit.add_data(&data[pos..]);
                    if added >= 0x100_0000 {
                        self.access_unit.clear_size();
                    }
                }
                break;
            }
        }

        self.sync_state = sync_state;
        found
    }
}

impl Default for H265Parser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── テスト用ビット列ライター ──

    struct BitWriter {
        bits: Vec<bool>,
    }

    impl BitWriter {
        fn new() -> Self { Self { bits: Vec::new() } }

        fn push_u(&mut self, v: u32, n: usize) {
            for i in (0..n).rev() {
                self.bits.push((v >> i) & 1 != 0);
            }
        }

        fn push_flag(&mut self, f: bool) {
            self.bits.push(f);
        }

        fn push_ue_v(&mut self, v: u32) {
            if v == 0 { self.bits.push(true); return; }
            let n = (v + 1).ilog2() as usize;
            for _ in 0..n { self.bits.push(false); }
            let val = v + 1;
            for i in (0..=n).rev() {
                self.bits.push((val >> i) & 1 != 0);
            }
        }

        /// RBSP バイト列を返す(emulation prevention なし)
        fn to_bytes(&self) -> Vec<u8> {
            let mut out = Vec::new();
            for chunk in self.bits.chunks(8) {
                let mut b = 0u8;
                for (i, &bit) in chunk.iter().enumerate() {
                    if bit { b |= 1 << (7 - i); }
                }
                out.push(b);
            }
            out
        }

        /// EBSP バイト列を返す(0x000000/01/02 の前に 0x03 を挿入)
        fn to_ebsp(&self) -> Vec<u8> {
            let rbsp = self.to_bytes();
            let mut out = Vec::new();
            let mut zeros = 0usize;
            for &b in &rbsp {
                if zeros == 2 && b <= 0x03 {
                    out.push(0x03); // emulation prevention byte
                    zeros = 0;
                }
                out.push(b);
                if b == 0x00 { zeros += 1; } else { zeros = 0; }
            }
            out
        }
    }

    // ── find_next_start_code ──

    #[test]
    fn test_find_start_code_found() {
        let data = [0x00u8, 0x00, 0x01, 0xAB, 0xCD];
        // sync hits 0x000001 at i=2(after 3 bytes consumed) → Some(3)
        let r = find_next_start_code(&data, 0, data.len());
        assert_eq!(r, Some(3));
    }

    #[test]
    fn test_find_start_code_not_found() {
        let data = [0xFFu8, 0xFF, 0xFF, 0xFF];
        assert_eq!(find_next_start_code(&data, 0, data.len()), None);
    }

    // ── H264AccessUnit::parse_header ──

    /// 最小 H.264 SPS NAL ユニットを含むアクセスユニットを構築する。
    fn make_h264_sps_au(
        profile_idc: u8,
        level_idc: u8,
        width_mbs: u32,
        height_mbu: u32,
        frame_mbs_only: bool,
    ) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.push_u(profile_idc as u32, 8);
        w.push_flag(false); w.push_flag(false); w.push_flag(false); w.push_flag(false); // constraint_set0-3
        w.push_u(0, 4);  // reserved_zero_4bits
        w.push_u(level_idc as u32, 8);
        w.push_ue_v(0); // seq_parameter_set_id
        w.push_ue_v(0); // log2_max_frame_num_minus4
        w.push_ue_v(0); // pic_order_cnt_type=0
        w.push_ue_v(0); // log2_max_pic_order_cnt_lsb_minus4
        w.push_ue_v(1); // num_ref_frames
        w.push_flag(false); // gaps_in_frame_num_value_allowed_flag
        w.push_ue_v(width_mbs);
        w.push_ue_v(height_mbu);
        w.push_flag(frame_mbs_only);
        if !frame_mbs_only { w.push_flag(false); }
        w.push_flag(true);  // direct_8x8_inference_flag
        w.push_flag(false); // frame_cropping_flag
        w.push_flag(false); // vui_parameters_present_flag
        let sps_bytes = w.to_ebsp(); // EBSP (emulation prevention bytes 挿入済み)

        let mut au = vec![0x00u8, 0x00, 0x01, 0x67]; // NAL type 7 = SPS
        au.extend_from_slice(&sps_bytes);
        au.extend_from_slice(&[0x00, 0x00, 0x01, 0x09, 0xE0]); // AUD
        au
    }

    #[test]
    fn test_h264_parse_header_basic() {
        // 1280x720: mbs_minus1=79, map_units_minus1=44, frame_mbs_only=true
        let au_data = make_h264_sps_au(66, 31, 79, 44, true);
        let mut au = H264AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header(), "parse_header should succeed");
        assert!(au.found_sps);
        assert_eq!(au.header.sps.profile_idc, 66);
        assert_eq!(au.header.sps.level_idc, 31);
    }

    #[test]
    fn test_h264_get_horizontal_size() {
        // width = (mbs_minus1+1)*16 = 80*16 = 1280
        let au_data = make_h264_sps_au(66, 31, 79, 44, true);
        let mut au = H264AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header());
        assert_eq!(au.get_horizontal_size(), 1280);
    }

    #[test]
    fn test_h264_get_vertical_size() {
        // height = (44+1)*16 = 720
        let au_data = make_h264_sps_au(66, 31, 79, 44, true);
        let mut au = H264AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header());
        assert_eq!(au.get_vertical_size(), 720);
    }

    #[test]
    fn test_h264_parse_header_too_short() {
        let mut au = H264AccessUnit::new();
        au.set_data(&[0x00, 0x00, 0x01]);
        assert!(!au.parse_header());
    }

    #[test]
    fn test_h264_get_sar_no_vui() {
        let au_data = make_h264_sps_au(66, 31, 79, 44, true);
        let mut au = H264AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header());
        assert_eq!(au.get_sar(), None);
    }

    #[test]
    fn test_h264_get_timing_info_no_vui() {
        let au_data = make_h264_sps_au(66, 31, 79, 44, true);
        let mut au = H264AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header());
        assert_eq!(au.get_timing_info(), None);
    }

    #[test]
    fn test_h264_reset() {
        let au_data = make_h264_sps_au(66, 31, 79, 44, true);
        let mut au = H264AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header());
        au.reset();
        assert!(!au.found_sps);
    }

    // ── H264Parser::store_es ──

    /// SPS NAL + ダミー PPS NAL を含むアクセスユニット(SPS の後に次のスタートコードあり)
    fn make_h264_full_au(profile_idc: u8, level_idc: u8, w: u32, h: u32) -> Vec<u8> {
        let sps_au = make_h264_sps_au(profile_idc, level_idc, w, h, true);
        // AUD の前に PPS ダミー [0x00,0x00,0x01,0x68,0xCE,0x38,0x80] を挿入
        let aud_start = sps_au.len() - 5;
        let mut full = sps_au[..aud_start].to_vec(); // SPS まで
        full.extend_from_slice(&[0x00, 0x00, 0x01, 0x68, 0xCE]); // PPS NAL ダミー
        full.extend_from_slice(&sps_au[aud_start..]); // AUD
        full
    }

    #[test]
    fn test_h264_store_es_with_pps() {
        // SPS + PPS + AUD を含む完全 AU を 3 つ連結; 2nd AU 以降が handler で受け取れる
        let mut parser = H264Parser::new();
        let au = make_h264_full_au(66, 31, 79, 44);
        let mut es = au.clone();
        es.extend_from_slice(&au);
        es.extend_from_slice(&au);
        let mut called = 0u32;
        parser.store_es(&es, &mut |_: &H264AccessUnit| called += 1);
        assert!(called >= 1, "handler should be called at least once");
    }

    #[test]
    fn test_h264_store_es_parse_direct() {
        // parse_header を直接呼ぶ(store_es のスタートコード依存なし)
        let au_data = make_h264_sps_au(66, 31, 79, 44, true);
        // AU の後ろに別の AU の開始を追加することで内部の SPS → 次スタートコードを確保
        let mut au_data2 = au_data.clone();
        au_data2.extend_from_slice(&make_h264_sps_au(66, 31, 39, 29, true)[..4]); // 先頭スタートコード分
        let mut au = H264AccessUnit::new();
        au.set_data(&au_data2);
        assert!(au.parse_header());
        assert!(au.found_sps);
    }

    #[test]
    fn test_h264_store_es_no_start_code() {
        let mut parser = H264Parser::new();
        let mut called = 0u32;
        let found = parser.store_es(&[0xFF, 0xFF, 0xFF, 0xFF], &mut |_: &H264AccessUnit| called += 1);
        assert!(!found);
        assert_eq!(called, 0);
    }

    // ── H265AccessUnit ──

    /// 最小 H.265 SPS NAL ユニットを含むアクセスユニット。
    fn make_h265_sps_au(width: u32, height: u32) -> Vec<u8> {
        let mut w = BitWriter::new();
        w.push_u(0, 4);   // sps_video_parameter_set_id
        w.push_u(0, 3);   // sps_max_sub_layers_minus1=0
        w.push_flag(true); // sps_temporal_id_nesting_flag
        // profile_tier_level (general only, sps_max_sub_layers_minus1=0)
        w.push_u(0, 2);   // general_profile_space
        w.push_flag(false); // general_tier_flag
        w.push_u(1, 5);   // general_profile_idc=1 (Main)
        for _ in 0..32 { w.push_flag(false); } // general_profile_compatibility_flag
        w.push_flag(true);  // general_progressive_source_flag
        w.push_flag(false); // general_interlaced_source_flag
        w.push_flag(false); // general_non_packed_constraint_flag
        w.push_flag(false); // general_frame_only_constraint_flag
        for _ in 0..44 { w.push_flag(false); } // general_reserved_zero_44bits
        w.push_u(51, 8);  // general_level_idc=51 (Level 5.1)
        // no sub_layer (sps_max_sub_layers_minus1=0)

        w.push_ue_v(0);   // sps_seq_parameter_set_id
        w.push_ue_v(1);   // chroma_format_idc=1 (4:2:0)
        w.push_ue_v(width);
        w.push_ue_v(height);
        w.push_flag(false); // conformance_window_flag
        w.push_ue_v(0);   // bit_depth_luma_minus8
        w.push_ue_v(0);   // bit_depth_chroma_minus8
        w.push_ue_v(4);   // log2_max_pic_order_cnt_lsb_minus4
        w.push_flag(false); // sps_sub_layer_ordering_info_present_flag
        // sub_layer_ordering_info for i=0 (sps_max_sub_layers_minus1=0)
        w.push_ue_v(1);   // sps_max_dec_pic_buffering_minus1[0]
        w.push_ue_v(0);   // sps_max_num_reorder_pics[0]
        w.push_ue_v(0);   // sps_max_latency_increase_plus1[0]
        w.push_ue_v(0);   // log2_min_luma_coding_block_size_minus3
        w.push_ue_v(2);   // log2_diff_max_min_luma_coding_block_size
        w.push_ue_v(0);   // log2_min_transform_block_size_minus2
        w.push_ue_v(3);   // log2_diff_max_min_transform_block_size
        w.push_ue_v(1);   // max_transform_hierarchy_depth_inter
        w.push_ue_v(1);   // max_transform_hierarchy_depth_intra
        w.push_flag(false); // scaling_list_enabled_flag
        w.push_flag(false); // amp_enabled_flag
        w.push_flag(false); // sample_adaptive_offset_enabled_flag
        w.push_flag(false); // pcm_enabled_flag
        w.push_ue_v(0);   // num_short_term_ref_pic_sets
        w.push_flag(false); // long_term_ref_pics_present_flag
        w.push_flag(false); // sps_temporal_mvp_enabled_flag
        w.push_flag(false); // strong_intra_smoothing_enabled_flag
        w.push_flag(false); // vui_parameters_present_flag
        let sps_bytes = w.to_ebsp(); // EBSP (emulation prevention bytes 挿入済み)

        // AU = 0x000001 | SPS NAL header [0x42,0x01] | sps_bytes | AUD NAL
        // nal_unit_header: byte0=(0x21<<1)=0x42, byte1=nuh_layer_id<<3|nuh_temporal_id_plus1=0x01
        let mut au = vec![0x00u8, 0x00, 0x01, 0x42, 0x01];
        au.extend_from_slice(&sps_bytes);
        // AUD: nal_unit_type=0x23, byte0=(0x23<<1)=0x46, byte1=0x01
        au.extend_from_slice(&[0x00, 0x00, 0x01, 0x46, 0x01, 0xE0]);
        au
    }

    #[test]
    fn test_h265_parse_header_basic() {
        let au_data = make_h265_sps_au(1920, 1080);
        let mut au = H265AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header(), "H.265 parse_header should succeed");
        assert!(au.found_sps);
    }

    #[test]
    fn test_h265_get_horizontal_size() {
        let au_data = make_h265_sps_au(1920, 1080);
        let mut au = H265AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header());
        assert_eq!(au.get_horizontal_size(), 1920);
    }

    #[test]
    fn test_h265_get_vertical_size() {
        let au_data = make_h265_sps_au(1920, 1080);
        let mut au = H265AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header());
        assert_eq!(au.get_vertical_size(), 1080);
    }

    #[test]
    fn test_h265_parse_header_too_short() {
        let mut au = H265AccessUnit::new();
        au.set_data(&[0x00, 0x00, 0x01, 0x42]);
        assert!(!au.parse_header());
    }

    #[test]
    fn test_h265_get_sar_no_vui() {
        let au_data = make_h265_sps_au(1920, 1080);
        let mut au = H265AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header());
        assert_eq!(au.get_sar(), None);
    }

    #[test]
    fn test_h265_get_timing_info_no_vui() {
        let au_data = make_h265_sps_au(1920, 1080);
        let mut au = H265AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header());
        assert_eq!(au.get_timing_info(), None);
    }

    #[test]
    fn test_h265_reset() {
        let au_data = make_h265_sps_au(1920, 1080);
        let mut au = H265AccessUnit::new();
        au.set_data(&au_data);
        assert!(au.parse_header());
        au.reset();
        assert!(!au.found_sps);
    }

    /// H265 フル AU: SPS + ダミー PPS NAL + AUD
    fn make_h265_full_au(width: u32, height: u32) -> Vec<u8> {
        let sps_au = make_h265_sps_au(width, height);
        // AUD (6 bytes) の前に PPS ダミーを挿入
        // AUD は末尾 6 バイト [0x00,0x00,0x01,0x46,0x01,0xE0]
        let aud_start = sps_au.len() - 6;
        let mut full = sps_au[..aud_start].to_vec();
        // PPS NAL: nal_type=0x22, header=[0x44,0x01] + 1 byte payload
        full.extend_from_slice(&[0x00, 0x00, 0x01, 0x44, 0x01, 0xC0]);
        full.extend_from_slice(&sps_au[aud_start..]);
        full
    }

    #[test]
    fn test_h265_store_es_with_pps() {
        // SPS + PPS + AUD を含む完全 AU を 3 つ連結
        let mut parser = H265Parser::new();
        let au = make_h265_full_au(1920, 1080);
        let mut es = au.clone();
        es.extend_from_slice(&au);
        es.extend_from_slice(&au);
        let mut called = 0u32;
        parser.store_es(&es, &mut |_: &H265AccessUnit| called += 1);
        assert!(called >= 1, "handler should be called at least once");
    }

    #[test]
    fn test_h265_store_es_parse_direct() {
        // SPS の後ろに別の AUD スタートコードを追加して parse_header を通す
        let au_data = make_h265_sps_au(1920, 1080);
        let mut combined = au_data.clone();
        combined.extend_from_slice(&make_h265_sps_au(1920, 1080)[..5]); // 先頭の start_code+nal_header
        let mut au = H265AccessUnit::new();
        au.set_data(&combined);
        assert!(au.parse_header());
        assert!(au.found_sps);
    }
}
