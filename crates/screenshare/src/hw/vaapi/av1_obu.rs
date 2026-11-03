//! The AV1 headers a VA-API AV1 encoder expects from the application: a sequence header OBU and
//! a frame header OBU per frame, sent as "packed headers". The driver writes the tile data and
//! patches the fields its rate control decides (base_q_idx, loop filter, CDEF, obu_size) at the
//! bit offsets reported here.
//!
//! Section numbers refer to the AV1 Bitstream & Decoding Process Specification (v1.0.0 with
//! errata 1). Only the small subset this encoder uses is written: 8-bit 4:2:0 (profile 0), one
//! operating point, no screen-content tools, no superres, no loop restoration, one tile, and one
//! reference per inter frame.

/// Bits of `order_hint` (`order_hint_bits_minus_1` + 1).
pub(super) const ORDER_HINT_BITS: u32 = 8;

/// `primary_ref_frame` value for "no primary reference" (§6.8.2).
pub(super) const PRIMARY_REF_NONE: u8 = 7;

/// Placeholder `base_q_idx`; the driver overwrites it with its rate control's choice.
pub(super) const BASE_Q_IDX: u8 = 128;

/// Largest frame this encoder signals with a single tile: MAX_TILE_WIDTH and MAX_TILE_AREA
/// (§3), so the tile info syntax always selects one tile.
pub(super) const MAX_WIDTH: u32 = 4096;
pub(super) const MAX_AREA: u32 = 4096 * 2304;

const OBU_SEQUENCE_HEADER: u8 = 1;
const OBU_TEMPORAL_DELIMITER: u8 = 2;
const OBU_FRAME_HEADER: u8 = 3;

/// A temporal delimiter OBU with an empty payload (§5.6), which starts every temporal unit.
pub(super) const TEMPORAL_DELIMITER: [u8; 2] = [(OBU_TEMPORAL_DELIMITER << 3) | 0b10, 0];

/// MSB-first bit writer (§4.10.2 f(n)).
#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    bits: usize,
}

impl BitWriter {
    fn put(&mut self, value: u32, bits: u32) {
        for i in (0..bits).rev() {
            if self.bits.is_multiple_of(8) {
                self.bytes.push(0);
            }
            if (value >> i) & 1 == 1 {
                let last = self.bytes.len() - 1;
                self.bytes[last] |= 0x80 >> (self.bits % 8);
            }
            self.bits += 1;
        }
    }

    fn flag(&mut self, value: bool) {
        self.put(u32::from(value), 1);
    }

    fn position(&self) -> u32 {
        self.bits as u32
    }

    /// trailing_bits() (§5.3.4): a one, then zeros up to the byte boundary.
    fn trailing_bits(&mut self) {
        self.put(1, 1);
        while !self.bits.is_multiple_of(8) {
            self.put(0, 1);
        }
    }
}

/// leb128() (§4.10.5) padded to exactly `len` bytes, so the driver can rewrite the value in
/// place.
fn leb128_fixed(value: usize, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| {
            let byte = ((value >> (7 * i)) & 0x7f) as u8;
            if i + 1 < len { byte | 0x80 } else { byte }
        })
        .collect()
}

/// obu_header() (§5.3.2) without extension, with obu_has_size_field = 1.
fn obu_header(obu_type: u8) -> u8 {
    (obu_type << 3) | 0b10
}

/// The level (§A.3) a stream of this size and rate needs: the smallest whose picture size,
/// dimensions, display rate and main-tier bitrate all fit. `seq_level_idx` = 2 × (major − 2) +
/// minor.
pub(super) fn level_idx(width: u32, height: u32, fps: u32, bitrate_kbps: u32) -> u8 {
    // (seq_level_idx, MaxPicSize, MaxHSize, MaxVSize, MaxDisplayRate, MaxBitrate main tier in
    // kbit/s); the defined levels from Annex A.3, table "General tier and level limits".
    const LEVELS: [(u8, u64, u32, u32, u64, u32); 10] = [
        (0, 147_456, 2048, 1152, 4_423_680, 1_500),
        (1, 278_784, 2816, 1584, 8_363_520, 3_000),
        (4, 665_856, 4352, 2448, 19_975_680, 6_000),
        (5, 1_065_024, 5504, 3096, 31_950_720, 10_000),
        (8, 2_359_296, 6144, 3456, 70_778_880, 12_000),
        (9, 2_359_296, 6144, 3456, 141_557_760, 20_000),
        (12, 8_912_896, 8192, 4352, 267_386_880, 30_000),
        (13, 8_912_896, 8192, 4352, 534_773_760, 40_000),
        (14, 8_912_896, 8192, 4352, 1_069_547_520, 60_000),
        (16, 35_651_584, 16384, 8704, 1_069_547_520, 60_000),
    ];
    let pic = u64::from(width) * u64::from(height);
    LEVELS
        .iter()
        .find(|(_, max_pic, max_w, max_h, max_rate, max_kbps)| {
            pic <= *max_pic
                && width <= *max_w
                && height <= *max_h
                && pic * u64::from(fps) <= *max_rate
                && bitrate_kbps <= *max_kbps
        })
        // Level 31 is "no level constraints" (§6.4.1).
        .map_or(31, |level| level.0)
}

/// Number of bits needed to code `value` (at least 1).
fn bits_for(value: u32) -> u32 {
    (u32::BITS - value.leading_zeros()).max(1)
}

/// sequence_header_obu() (§5.5) for an 8-bit 4:2:0 stream of at most `width`×`height`.
pub(super) fn sequence_header(width: u32, height: u32, level_idx: u8) -> Vec<u8> {
    let mut w = BitWriter::default();
    w.put(0, 3); // seq_profile: Main (8-bit 4:2:0)
    w.flag(false); // still_picture
    w.flag(false); // reduced_still_picture_header
    w.flag(false); // timing_info_present_flag
    w.flag(false); // initial_display_delay_present_flag
    w.put(0, 5); // operating_points_cnt_minus_1
    w.put(0, 12); // operating_point_idc[0]: all layers
    w.put(u32::from(level_idx), 5); // seq_level_idx[0]
    if level_idx > 7 {
        w.flag(false); // seq_tier[0]: Main
    }
    let width_bits = bits_for(width - 1);
    let height_bits = bits_for(height - 1);
    w.put(width_bits - 1, 4); // frame_width_bits_minus_1
    w.put(height_bits - 1, 4); // frame_height_bits_minus_1
    w.put(width - 1, width_bits); // max_frame_width_minus_1
    w.put(height - 1, height_bits); // max_frame_height_minus_1
    w.flag(false); // frame_id_numbers_present_flag
    w.flag(false); // use_128x128_superblock
    w.flag(false); // enable_filter_intra
    w.flag(false); // enable_intra_edge_filter
    w.flag(false); // enable_interintra_compound
    w.flag(false); // enable_masked_compound
    w.flag(false); // enable_warped_motion
    w.flag(false); // enable_dual_filter
    w.flag(true); // enable_order_hint
    w.flag(false); // enable_jnt_comp
    w.flag(false); // enable_ref_frame_mvs
    // seq_choose_screen_content_tools = 0 and seq_force_screen_content_tools = 0: frames never
    // code allow_screen_content_tools (and so never force_integer_mv).
    w.flag(false);
    w.put(0, 1);
    w.put(ORDER_HINT_BITS - 1, 3); // order_hint_bits_minus_1
    w.flag(false); // enable_superres
    w.flag(true); // enable_cdef
    w.flag(false); // enable_restoration
    // color_config() (§5.5.2) for profile 0: 8-bit, 4:2:0 implied.
    w.flag(false); // high_bitdepth
    w.flag(false); // mono_chrome
    w.flag(false); // color_description_present_flag
    w.flag(false); // color_range: studio swing
    w.put(0, 2); // chroma_sample_position: unknown
    w.flag(false); // separate_uv_delta_q
    w.flag(false); // film_grain_params_present
    w.trailing_bits();

    let mut obu = vec![obu_header(OBU_SEQUENCE_HEADER)];
    obu.extend(leb128_fixed(w.bytes.len(), 1));
    obu.extend(w.bytes);
    obu
}

/// How one frame refers to the eight reference slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrameRefs {
    /// A shown key frame: no reference, refreshes every slot.
    Key,
    /// An inter frame predicted from slot `reference` (for every reference name), which stores
    /// itself into the slots set in `refresh`.
    Inter { reference: u8, refresh: u8 },
}

/// The packed frame header and the positions the driver patches.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct FrameHeader {
    pub data: Vec<u8>,
    pub bit_offset_qindex: u32,
    pub bit_offset_segmentation: u32,
    pub bit_offset_loopfilter_params: u32,
    pub bit_offset_cdef_params: u32,
    pub size_in_bits_cdef_params: u32,
    /// Byte position of the obu_size field within `data`.
    pub byte_offset_obu_size: u32,
}

/// Superblock columns and rows of a frame with 64×64 superblocks (§5.9.2 compute_image_size,
/// §5.9.15 tile_info).
pub(super) fn superblocks(width: u32, height: u32) -> (u32, u32) {
    let mi_cols = 2 * width.div_ceil(8);
    let mi_rows = 2 * height.div_ceil(8);
    (mi_cols.div_ceil(16), mi_rows.div_ceil(16))
}

/// tile_log2() (§5.9.16).
fn tile_log2(block: u32, target: u32) -> u32 {
    let mut k = 0;
    while (block << k) < target {
        k += 1;
    }
    k
}

/// frame_header_obu() (§5.9) as an OBU_FRAME_HEADER, with its obu_size field padded to
/// `obu_size_bytes` so the driver can patch it.
pub(super) fn frame_header(
    refs: FrameRefs,
    order_hint: u8,
    width: u32,
    height: u32,
    obu_size_bytes: usize,
) -> FrameHeader {
    let mut w = BitWriter::default();
    let key = refs == FrameRefs::Key;

    // uncompressed_header() (§5.9.2).
    w.flag(false); // show_existing_frame
    w.put(if key { 0 } else { 1 }, 2); // frame_type: KEY_FRAME or INTER_FRAME
    w.flag(true); // show_frame
    if !key {
        // A shown key frame is error resilient implicitly; inter frames rely on their reference.
        w.flag(false); // error_resilient_mode
    }
    w.flag(false); // disable_cdf_update
    w.flag(false); // frame_size_override_flag
    w.put(u32::from(order_hint), ORDER_HINT_BITS);
    if let FrameRefs::Inter { reference, refresh } = refs {
        // CDFs and loop filter deltas come from LAST_FRAME, the frame's only reference, so a
        // stream thinned to lower temporal layers still has everything it loads.
        w.put(0, 3); // primary_ref_frame: LAST_FRAME
        w.put(u32::from(refresh), 8); // refresh_frame_flags
        w.flag(false); // frame_refs_short_signaling
        for _ in 0..7 {
            w.put(u32::from(reference), 3); // ref_frame_idx[i]
        }
        // frame_size() and superres_params() code nothing without frame_size_override_flag
        // and enable_superres; render_size():
        w.flag(false); // render_and_frame_size_different
        w.flag(false); // allow_high_precision_mv
        // read_interpolation_filter() (§5.9.10)
        w.flag(false); // is_filter_switchable
        w.put(0, 2); // interpolation_filter: EIGHTTAP
        w.flag(false); // is_motion_mode_switchable
    } else {
        // frame_size() and superres_params() code nothing; render_size():
        w.flag(false); // render_and_frame_size_different
    }
    w.flag(false); // disable_frame_end_update_cdf

    // tile_info() (§5.9.15): uniform spacing with the fewest tiles allowed, which is one tile
    // for frames up to MAX_WIDTH × MAX_AREA.
    let (sb_cols, sb_rows) = superblocks(width, height);
    let max_log2_tile_cols = tile_log2(1, sb_cols.min(64));
    let max_log2_tile_rows = tile_log2(1, sb_rows.min(64));
    let min_log2_tile_cols = tile_log2(MAX_WIDTH / 64, sb_cols);
    debug_assert_eq!(min_log2_tile_cols, 0, "frame too wide for one tile");
    w.flag(true); // uniform_tile_spacing_flag
    if min_log2_tile_cols < max_log2_tile_cols {
        w.flag(false); // increment_tile_cols_log2
    }
    if max_log2_tile_rows > 0 {
        w.flag(false); // increment_tile_rows_log2
    }

    // quantization_params() (§5.9.12)
    let bit_offset_qindex = w.position();
    w.put(u32::from(BASE_Q_IDX), 8); // base_q_idx
    w.flag(false); // DeltaQYDc: delta_coded
    w.flag(false); // DeltaQUDc: delta_coded
    w.flag(false); // DeltaQUAc: delta_coded
    w.flag(false); // using_qmatrix

    // segmentation_params() (§5.9.14)
    let bit_offset_segmentation = w.position();
    w.flag(false); // segmentation_enabled

    // delta_q_params() (§5.9.17); base_q_idx > 0 so the flag is coded. delta_lf_params()
    // codes nothing without delta_q_present.
    w.flag(false); // delta_q_present

    // loop_filter_params() (§5.9.11); the driver fills in its levels.
    let bit_offset_loopfilter_params = w.position();
    w.put(0, 6); // loop_filter_level[0]
    w.put(0, 6); // loop_filter_level[1]
    w.put(0, 3); // loop_filter_sharpness
    w.flag(false); // loop_filter_delta_enabled

    // cdef_params() (§5.9.19); the driver fills in its strengths.
    let bit_offset_cdef_params = w.position();
    w.put(0, 2); // cdef_damping_minus_3
    w.put(0, 2); // cdef_bits
    w.put(0, 4); // cdef_y_pri_strength[0]
    w.put(0, 2); // cdef_y_sec_strength[0]
    w.put(0, 4); // cdef_uv_pri_strength[0]
    w.put(0, 2); // cdef_uv_sec_strength[0]
    let size_in_bits_cdef_params = w.position() - bit_offset_cdef_params;

    // lr_params() codes nothing without enable_restoration. read_tx_mode() (§5.9.21):
    w.flag(true); // tx_mode_select: TX_MODE_SELECT
    if !key {
        w.flag(false); // reference_select: single reference
        // skip_mode_params() codes nothing without reference_select; allow_warped_motion is
        // not coded without enable_warped_motion.
    }
    w.flag(false); // reduced_tx_set
    if !key {
        // global_motion_params() (§5.9.24)
        for _ in 0..7 {
            w.flag(false); // is_global
        }
    }
    // film_grain_params() codes nothing without film_grain_params_present.
    w.trailing_bits();

    let mut data = vec![obu_header(OBU_FRAME_HEADER)];
    data.extend(leb128_fixed(w.bytes.len(), obu_size_bytes));
    let prefix_bits = 8 * data.len() as u32;
    data.extend(w.bytes);
    FrameHeader {
        data,
        bit_offset_qindex: prefix_bits + bit_offset_qindex,
        bit_offset_segmentation: prefix_bits + bit_offset_segmentation,
        bit_offset_loopfilter_params: prefix_bits + bit_offset_loopfilter_params,
        bit_offset_cdef_params: prefix_bits + bit_offset_cdef_params,
        size_in_bits_cdef_params,
        byte_offset_obu_size: 1,
    }
}

/// Whether `data` starts a temporal unit with a temporal delimiter OBU.
pub(super) fn starts_with_temporal_delimiter(data: &[u8]) -> bool {
    data.first()
        .is_some_and(|byte| (byte >> 3) & 0xf == OBU_TEMPORAL_DELIMITER)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(data: &[u8]) -> String {
        data.iter().map(|b| format!("{b:02x}")).collect()
    }

    // The expected bytes are what a VA-API driver accepted for a 1280×720 stream (captured
    // with LIBVA_TRACE from another encoder on the development machine), so the headers match
    // what drivers parse and patch, bit for bit.
    #[test]
    fn sequence_header_for_720p() {
        assert_eq!(level_idx(1280, 720, 30, 2000), 5);
        assert_eq!(
            hex(&sequence_header(1280, 720, 5)),
            "0a0b0000002d4cffb3c021d004"
        );
    }

    #[test]
    fn key_frame_header_for_720p() {
        let header = frame_header(FrameRefs::Key, 0, 1280, 720, 2);
        assert_eq!(hex(&header.data), "1a8900100090000000000050");
        assert_eq!(header.bit_offset_qindex, 43);
        assert_eq!(header.bit_offset_loopfilter_params, 57);
        assert_eq!(header.bit_offset_cdef_params, 73);
        assert_eq!(header.size_in_bits_cdef_params, 16);
        assert_eq!(header.byte_offset_obu_size, 1);
    }

    #[test]
    fn inter_frame_header_for_720p() {
        let refs = FrameRefs::Inter {
            reference: 0,
            refresh: 0b10,
        };
        let header = frame_header(refs, 1, 1280, 720, 2);
        assert_eq!(hex(&header.data), "1a8f00300200800000012000000000008020");
        assert_eq!(header.bit_offset_qindex, 82);
        assert_eq!(header.bit_offset_loopfilter_params, 96);
        assert_eq!(header.bit_offset_cdef_params, 112);
    }

    #[test]
    fn reference_slot_and_size_field_are_coded() {
        let refs = FrameRefs::Inter {
            reference: 5,
            refresh: 0,
        };
        let header = frame_header(refs, 3, 1920, 1080, 4);
        // A 4-byte obu_size moves everything after it by two more bytes than above.
        assert_eq!(&header.data[1..5], &[0x8f, 0x80, 0x80, 0x00]);
        assert_eq!(header.bit_offset_qindex, 82 + 16);
        // ref_frame_idx starts after 27 payload bits: 7 × 0b101.
        let payload = &header.data[5..];
        let bits: String = payload.iter().map(|b| format!("{b:08b}")).collect();
        assert_eq!(&bits[27..48], "101".repeat(7));
        assert_eq!(&bits[7..15], "00000011", "order_hint");
        assert_eq!(&bits[18..26], "00000000", "refresh_frame_flags");
    }

    #[test]
    fn levels_follow_size_rate_and_bitrate() {
        assert_eq!(level_idx(1920, 1080, 30, 6000), 8);
        assert_eq!(level_idx(1920, 1080, 60, 6000), 9);
        assert_eq!(level_idx(2560, 1440, 60, 12000), 12);
        assert_eq!(
            level_idx(1280, 720, 30, 15000),
            9,
            "bitrate raises the level"
        );
        assert_eq!(level_idx(8192, 8192, 60, 1000), 31);
    }

    #[test]
    fn padded_leb128() {
        assert_eq!(leb128_fixed(9, 2), [0x89, 0x00]);
        assert_eq!(leb128_fixed(300, 4), [0xac, 0x82, 0x80, 0x00]);
        assert_eq!(leb128_fixed(11, 1), [0x0b]);
    }

    #[test]
    fn temporal_delimiter_detection() {
        assert!(starts_with_temporal_delimiter(&TEMPORAL_DELIMITER));
        assert!(!starts_with_temporal_delimiter(&[0x0a, 0x0b]));
        assert!(!starts_with_temporal_delimiter(&[]));
    }
}
