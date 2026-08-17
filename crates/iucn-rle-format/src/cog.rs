//! Reading Cloud-Optimized `GeoTIFF` windows over byte ranges.
//!
//! A COG is a TIFF arranged so a client can read its header, work out which tiles it
//! wants, and fetch only those. This module does exactly that, synchronously, over
//! bytes someone else fetched.
//!
//! # Identifying the projection
//!
//! The AOO grid is defined in ESRI:54034, and a raster in any other CRS must be
//! refused rather than silently used — pixels only stay rectangular in the grid's own
//! plane. Checking that turns out **not** to be a code lookup: GDAL writes ESRI:54034
//! with `ProjectedCSTypeGeoKey = 32767` ("user-defined"), so the file carries no EPSG
//! or ESRI code at all. The CRS is identifiable only from the coordinate
//! transformation and its parameters — Cylindrical Equal Area, standard parallel 0,
//! central meridian 0, no false offsets, on the WGS 84 ellipsoid.
//!
//! That same transformation code is what makes these files unreadable in the browser
//! today: `@developmentseed/geotiff` rejects them with *"Unsupported coordinate
//! transformation type: 28"*. Reading them is the capability this project exists to add.

use core::ops::Range;
use std::io::{Read, Seek, SeekFrom};

use tiff::decoder::{Decoder, DecodingResult};
use tiff::tags::Tag;

use crate::geoparquet::SparseBytes;

/// Bytes to fetch on the first request.
///
/// A COG's header — IFD, tile offset table, projection keys — is a few kilobytes for
/// typical rasters. Overshooting costs one small read; undershooting costs a round trip.
pub const DEFAULT_HEADER_PREFETCH: u64 = 16 * 1024;

/// Largest header this reader will chase before giving up.
///
/// A raster with very many tiles has a correspondingly large offset table, but a header
/// beyond this is likelier to be a malformed file than a real one, and chasing it would
/// mean downloading the whole object one doubling at a time.
const MAX_HEADER: u64 = 16 * 1024 * 1024;

/// `GeoTIFF` key numbers, from the `GeoTIFF` specification.
mod geokey {
    /// Whether the model is projected, geographic, or geocentric.
    pub const MODEL_TYPE: u16 = 1024;
    /// The projected CRS code, or 32767 for user-defined.
    pub const PROJECTED_CS_TYPE: u16 = 3072;
    /// Which coordinate transformation the projection uses.
    pub const PROJ_COORD_TRANS: u16 = 3075;
    /// Standard parallel, indexed into the double parameters.
    pub const STD_PARALLEL_1: u16 = 3078;
    /// Longitude of natural origin.
    pub const NAT_ORIGIN_LONG: u16 = 3080;
    /// Central meridian, an older spelling some writers use instead.
    pub const CENTRAL_MERIDIAN: u16 = 3088;
    /// False easting.
    pub const FALSE_EASTING: u16 = 3082;
    /// False northing.
    pub const FALSE_NORTHING: u16 = 3083;
    /// Semi-major axis of the ellipsoid.
    pub const SEMI_MAJOR_AXIS: u16 = 2057;
    /// Inverse flattening of the ellipsoid.
    pub const INV_FLATTENING: u16 = 2059;

    /// A projected coordinate system.
    pub const MODEL_TYPE_PROJECTED: u16 = 1;
    /// Cylindrical Equal Area — the transformation ESRI:54034 uses.
    pub const CT_CYLINDRICAL_EQUAL_AREA: u16 = 28;
}

/// WGS 84 ellipsoid parameters, which ESRI:54034 is defined on.
const WGS84_SEMI_MAJOR: f64 = 6_378_137.0;
const WGS84_INV_FLATTENING: f64 = 298.257_223_563;
/// Tolerance for comparing projection parameters read from a file.
///
/// Writers round these to varying precision, so exact equality would reject files that
/// are the projection in question. Tight enough that a genuinely different standard
/// parallel or central meridian still fails.
const PARAMETER_TOLERANCE: f64 = 1e-6;

/// Something that went wrong reading a COG.
#[derive(Debug, thiserror::Error)]
pub enum CogError {
    /// The bytes are not a TIFF at all.
    ///
    /// Usually a URL that returned an error page rather than a raster.
    #[error("not a TIFF: {reason}")]
    NotTiff {
        /// What was wrong with the bytes.
        reason: String,
    },

    /// The raster could not be read.
    #[error("could not read the raster: {reason}")]
    Read {
        /// The underlying failure.
        reason: String,
    },

    /// The raster uses a layout this reader does not handle.
    #[error("unsupported raster: {reason}")]
    Unsupported {
        /// What is unsupported.
        reason: String,
    },
}

/// A rectangle of pixels in the raster's own grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelWindow {
    /// Leftmost column.
    pub x: u32,
    /// Topmost row.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

impl PixelWindow {
    /// A window from its top-left corner and size.
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// This window clipped to a raster of the given size.
    fn clipped(self, width: u32, height: u32) -> Self {
        let x = self.x.min(width);
        let y = self.y.min(height);
        Self {
            x,
            y,
            width: self.width.min(width.saturating_sub(x)),
            height: self.height.min(height.saturating_sub(y)),
        }
    }

    fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// A decoded window of raster values, and where it sits in the world.
#[derive(Debug, Clone)]
pub struct Raster {
    /// Row-major pixel values, widened to `f64`.
    ///
    /// Class codes are small integers, exactly representable, and this is the type the
    /// accumulator takes — so widening here avoids a second pass later.
    pub values: Vec<f64>,
    /// Pixels per row.
    pub width: usize,
    /// Rows.
    pub height: usize,
    /// X of the window's outer edge, in projected metres.
    pub origin_x: f64,
    /// Y of the window's outer edge, in projected metres.
    pub origin_y: f64,
    /// Pixel width in projected metres.
    pub pixel_width: f64,
    /// Pixel height in projected metres, negative for a north-up raster.
    pub pixel_height: f64,
}

impl Raster {
    /// Coverage of one class: 1.0 where a pixel holds it, 0.0 elsewhere.
    ///
    /// This is how a categorical raster becomes something the AOO accumulator can take,
    /// which works in fractional coverage rather than class codes.
    ///
    /// Exact equality is deliberate: class codes are small integers held as `f64`, so
    /// they are exactly representable, and a tolerance would merge adjacent codes.
    #[must_use]
    #[allow(clippy::float_cmp)]
    pub fn mask(&self, class: f64) -> Vec<f64> {
        self.values
            .iter()
            .map(|&value| f64::from(u8::from(value == class)))
            .collect()
    }

    /// Borrow this window, with a coverage array, as the accumulator's input.
    #[must_use]
    pub fn as_window<'a>(&'a self, coverage: &'a [f64]) -> iucn_rle_core::aoo::RasterWindow<'a> {
        iucn_rle_core::aoo::RasterWindow {
            data: coverage,
            width: self.width,
            height: self.height,
            origin_x: self.origin_x,
            origin_y: self.origin_y,
            pixel_width: self.pixel_width,
            pixel_height: self.pixel_height,
        }
    }
}

/// An opened COG: everything the first image's header said.
#[derive(Debug)]
pub struct Cog {
    width: u32,
    height: u32,
    tile_width: u32,
    tile_height: u32,
    tile_offsets: Vec<u64>,
    tile_byte_counts: Vec<u64>,
    origin_x: f64,
    origin_y: f64,
    pixel_width: f64,
    pixel_height: f64,
    projection: Projection,
    header_length: u64,
}

/// What the `GeoTIFF` keys said about the projection.
#[derive(Debug, Clone, Default)]
struct Projection {
    model_type: Option<u16>,
    projected_cs: Option<u16>,
    transformation: Option<u16>,
    standard_parallel: Option<f64>,
    central_meridian: Option<f64>,
    false_easting: Option<f64>,
    false_northing: Option<f64>,
    semi_major: Option<f64>,
    inverse_flattening: Option<f64>,
}

impl Projection {
    /// Whether these keys describe the AOO grid's CRS.
    ///
    /// Checked by parameters rather than by code, because GDAL writes ESRI:54034 as
    /// user-defined and no code appears in the file.
    fn is_aoo_crs(&self) -> bool {
        let near = |value: Option<f64>, wanted: f64| {
            value.is_some_and(|found| (found - wanted).abs() < PARAMETER_TOLERANCE)
        };
        // Ellipsoid parameters are optional in the file; when absent the datum key
        // carries WGS 84, so their absence is not disqualifying. A *different* stated
        // ellipsoid is.
        let ellipsoid_ok = self
            .semi_major
            .is_none_or(|value| (value - WGS84_SEMI_MAJOR).abs() < 1.0)
            && self
                .inverse_flattening
                .is_none_or(|value| (value - WGS84_INV_FLATTENING).abs() < 1e-6);

        self.model_type == Some(geokey::MODEL_TYPE_PROJECTED)
            && self.transformation == Some(geokey::CT_CYLINDRICAL_EQUAL_AREA)
            && near(self.standard_parallel, 0.0)
            && near(self.central_meridian, 0.0)
            && near(self.false_easting, 0.0)
            && near(self.false_northing, 0.0)
            && ellipsoid_ok
    }

    fn describe(&self) -> String {
        format!(
            "model type {:?}, projected CS {:?}, transformation {:?}, standard parallel \
             {:?}, central meridian {:?}",
            self.model_type,
            self.projected_cs,
            self.transformation,
            self.standard_parallel,
            self.central_meridian
        )
    }
}

/// The outcome of parsing a header.
#[derive(Debug)]
pub enum Header {
    /// The header was complete in the bytes provided.
    Complete(Box<Cog>),
    /// More bytes are needed; fetch this range and parse again.
    NeedMore(Range<u64>),
}

/// The byte range to fetch first when opening a COG.
///
/// A COG puts its header at the start, which is what distinguishes it from an ordinary
/// TIFF, so this reads forward from zero rather than from the end as parquet does.
#[must_use]
pub fn header_range(file_size: u64, prefetch: u64) -> Range<u64> {
    0..prefetch.min(file_size)
}

/// Parse a COG header from the start of a file.
///
/// # Errors
///
/// See [`CogError`].
pub fn parse_header(head: &[u8], file_size: u64) -> Result<Header, CogError> {
    let source = PartialFile::new(head.to_vec(), file_size);
    let short = source.ran_short.clone();

    match describe(source) {
        Ok(mut cog) => {
            cog.header_length = head.len() as u64;
            Ok(Header::Complete(Box::new(cog)))
        }
        Err(error) => {
            // The decoder itself discovers how far the header reaches, by asking for a
            // byte that was not fetched. Doubling from there converges quickly and
            // needs no second implementation of the IFD layout to predict it.
            if short.get() && (head.len() as u64) < file_size {
                let wanted = (head.len() as u64)
                    .saturating_mul(2)
                    .max(DEFAULT_HEADER_PREFETCH)
                    .min(file_size);
                if wanted > head.len() as u64 && wanted <= MAX_HEADER {
                    return Ok(Header::NeedMore(0..wanted));
                }
            }
            Err(error)
        }
    }
}

/// Read the first image's description from a reader.
fn describe<R: Read + Seek>(source: R) -> Result<Cog, CogError> {
    let mut decoder = Decoder::new(source).map_err(|error| CogError::NotTiff {
        reason: error.to_string(),
    })?;

    // The first IFD is full resolution; the ones after it are overviews. Reading an
    // overview by mistake yields a plausible raster at the wrong scale, so this never
    // advances past the first image.
    let (width, height) = decoder.dimensions().map_err(|error| read_error(&error))?;
    let (tile_width, tile_height) = decoder.chunk_dimensions();

    let tile_offsets =
        decoder
            .get_tag_u64_vec(Tag::TileOffsets)
            .map_err(|_| CogError::Unsupported {
                reason: "the raster is stripped rather than tiled, so a window cannot be \
                     read without fetching whole rows of the image"
                    .to_owned(),
            })?;
    let tile_byte_counts = decoder
        .get_tag_u64_vec(Tag::TileByteCounts)
        .map_err(|error| read_error(&error))?;

    let (origin_x, origin_y, pixel_width, pixel_height) = geotransform(&mut decoder)?;

    Ok(Cog {
        width,
        height,
        tile_width,
        tile_height,
        tile_offsets,
        tile_byte_counts,
        origin_x,
        origin_y,
        pixel_width,
        pixel_height,
        projection: projection(&mut decoder),
        header_length: 0,
    })
}

fn read_error(error: &tiff::TiffError) -> CogError {
    CogError::Read {
        reason: error.to_string(),
    }
}

/// Origin and pixel size from the `ModelTiepoint` and `ModelPixelScale` tags.
fn geotransform<R: Read + Seek>(
    decoder: &mut Decoder<R>,
) -> Result<(f64, f64, f64, f64), CogError> {
    let scale = decoder
        .get_tag_f64_vec(Tag::ModelPixelScaleTag)
        .map_err(|_| CogError::Unsupported {
            reason: "the raster has no ModelPixelScale tag, so its pixels have no known \
                     size on the ground"
                .to_owned(),
        })?;
    let tiepoint = decoder
        .get_tag_f64_vec(Tag::ModelTiepointTag)
        .map_err(|_| CogError::Unsupported {
            reason: "the raster has no ModelTiepoint tag, so its position is unknown".to_owned(),
        })?;

    if scale.len() < 2 || tiepoint.len() < 6 {
        return Err(CogError::Unsupported {
            reason: "the raster's positioning tags are too short to describe a \
                     north-up grid"
                .to_owned(),
        });
    }

    // Tiepoint maps raster point (i, j, k) to model point (x, y, z). A north-up COG
    // ties raster (0, 0) to the top-left corner, and pixel height runs negative because
    // rows go north to south while y increases northward.
    Ok((tiepoint[3], tiepoint[4], scale[0], -scale[1]))
}

/// Read the `GeoTIFF` key directory.
///
/// A missing or malformed directory yields an empty [`Projection`], which fails the AOO
/// CRS check — the safe direction, since the alternative is treating an unknown
/// projection as the expected one.
fn projection<R: Read + Seek>(decoder: &mut Decoder<R>) -> Projection {
    let Ok(directory) = decoder.get_tag_u32_vec(Tag::GeoKeyDirectoryTag) else {
        return Projection::default();
    };
    let doubles = decoder
        .get_tag_f64_vec(Tag::GeoDoubleParamsTag)
        .unwrap_or_default();

    let mut found = Projection::default();
    if directory.len() < 4 {
        return found;
    }

    // Header is four shorts, then `count` entries of four: key, location, count, value.
    // A location of 0 means the value is inline; 34736 means it indexes the doubles.
    for entry in directory[4..].chunks_exact(4) {
        // Every field of a GeoKey entry is a SHORT; the tiff crate widens them to u32
        // when it reads the tag, so narrowing back is exact rather than lossy.
        let (Ok(key), Ok(location)) = (u16::try_from(entry[0]), u16::try_from(entry[1])) else {
            continue;
        };
        let value = entry[3];
        let inline = (location == 0).then(|| u16::try_from(value).ok()).flatten();
        let double = (location == 34736)
            .then(|| doubles.get(value as usize).copied())
            .flatten();

        match key {
            geokey::MODEL_TYPE => found.model_type = inline,
            geokey::PROJECTED_CS_TYPE => found.projected_cs = inline,
            geokey::PROJ_COORD_TRANS => found.transformation = inline,
            geokey::STD_PARALLEL_1 => found.standard_parallel = double,
            // Two spellings of the same thing; writers differ over which they emit.
            geokey::NAT_ORIGIN_LONG | geokey::CENTRAL_MERIDIAN => {
                found.central_meridian = double;
            }
            geokey::FALSE_EASTING => found.false_easting = double,
            geokey::FALSE_NORTHING => found.false_northing = double,
            geokey::SEMI_MAJOR_AXIS => found.semi_major = double,
            geokey::INV_FLATTENING => found.inverse_flattening = double,
            _ => {}
        }
    }
    found
}

impl Cog {
    /// Width in pixels, at full resolution.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels, at full resolution.
    #[must_use]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Tile width and height in pixels.
    #[must_use]
    pub fn tile_size(&self) -> (u32, u32) {
        (self.tile_width, self.tile_height)
    }

    /// Projected coordinates of the raster's top-left corner.
    #[must_use]
    pub fn origin(&self) -> (f64, f64) {
        (self.origin_x, self.origin_y)
    }

    /// Pixel width and height in projected units.
    #[must_use]
    pub fn pixel_size(&self) -> (f64, f64) {
        (self.pixel_width, self.pixel_height)
    }

    /// Whether this raster is in the AOO grid's CRS, ESRI:54034.
    ///
    /// Determined from the projection's parameters, not from a code — see the module
    /// documentation for why a code lookup cannot work here.
    #[must_use]
    pub fn is_aoo_crs(&self) -> bool {
        self.projection.is_aoo_crs()
    }

    /// A human-readable account of the projection keys, for error messages.
    #[must_use]
    pub fn crs_description(&self) -> String {
        self.projection.describe()
    }

    /// How many bytes the header occupied.
    #[must_use]
    pub fn header_length(&self) -> u64 {
        self.header_length
    }

    /// Tiles across the raster.
    fn tiles_across(&self) -> u32 {
        self.width.div_ceil(self.tile_width)
    }

    /// The tiles a window covers, in raster order.
    #[must_use]
    pub fn tiles_for(&self, window: PixelWindow) -> Vec<usize> {
        let window = window.clipped(self.width, self.height);
        if window.is_empty() {
            return Vec::new();
        }

        let across = self.tiles_across();
        let first_col = window.x / self.tile_width;
        let last_col = (window.x + window.width - 1) / self.tile_width;
        let first_row = window.y / self.tile_height;
        let last_row = (window.y + window.height - 1) / self.tile_height;

        let mut tiles = Vec::new();
        for row in first_row..=last_row {
            for col in first_col..=last_col {
                let index = (row * across + col) as usize;
                if index < self.tile_offsets.len() {
                    tiles.push(index);
                }
            }
        }
        tiles
    }

    /// Every byte range needed to decode those tiles, ascending and non-overlapping.
    ///
    /// The header is always included, because decoding re-reads the IFD to find where
    /// each tile lives. A caller reading several windows from one raster should hold on
    /// to the header bytes and add tiles to them, rather than refetching it each time.
    #[must_use]
    pub fn ranges_for(&self, tiles: &[usize]) -> Vec<Range<u64>> {
        let mut ranges: Vec<Range<u64>> = core::iter::once(0..self.header_length)
            .filter(|range| range.end > 0)
            .chain(tiles.iter().filter_map(|&tile| {
                let start = *self.tile_offsets.get(tile)?;
                let length = *self.tile_byte_counts.get(tile)?;
                Some(start..start + length)
            }))
            .collect();
        ranges.sort_by_key(|range| range.start);

        let mut merged: Vec<Range<u64>> = Vec::with_capacity(ranges.len());
        for range in ranges {
            match merged.last_mut() {
                // Adjacent tiles are usually contiguous on disk, so merging turns a run
                // of them into one request.
                Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        merged
    }

    /// Decode a window from bytes fetched for it.
    ///
    /// The window is clipped to the raster, so asking near an edge is ordinary rather
    /// than an error, and no phantom pixels are invented beyond it.
    ///
    /// # Errors
    ///
    /// [`CogError::Read`] if the bytes the tiles need were not supplied — never a blank
    /// raster, which would read as "no ecosystem here".
    pub fn decode(&self, window: PixelWindow, bytes: &SparseBytes) -> Result<Raster, CogError> {
        let window = window.clipped(self.width, self.height);
        let (width, height) = (window.width as usize, window.height as usize);
        let mut values = vec![0.0; width * height];

        if !window.is_empty() {
            // The header has to be present too: the decoder re-reads the IFD to learn
            // where each tile lives before it can decode one.
            let mut decoder =
                Decoder::new(SeekableSparse::new(bytes.clone())).map_err(|error| {
                    CogError::Read {
                        reason: error.to_string(),
                    }
                })?;
            let across = self.tiles_across();

            for tile in self.tiles_for(window) {
                let index = u32::try_from(tile).map_err(|_| CogError::Read {
                    reason: format!("tile index {tile} is out of range"),
                })?;
                let chunk = decoder.read_chunk(index).map_err(|error| CogError::Read {
                    reason: format!("tile {tile}: {error}"),
                })?;
                if matches!(chunk, DecodingResult::F16(_)) {
                    // Silently reading these as zero would look like empty ground.
                    return Err(CogError::Unsupported {
                        reason: "half-precision float rasters are not supported".to_owned(),
                    });
                }
                let (tile_col, tile_row) = (index % across, index / across);
                self.blit(&chunk, tile_col, tile_row, window, &mut values, width);
            }
        }

        Ok(Raster {
            values,
            width,
            height,
            origin_x: self.pixel_width.mul_add(f64::from(window.x), self.origin_x),
            origin_y: self
                .pixel_height
                .mul_add(f64::from(window.y), self.origin_y),
            pixel_width: self.pixel_width,
            pixel_height: self.pixel_height,
        })
    }

    /// Copy the part of one decoded tile that falls inside the window.
    ///
    /// The arithmetic here is where a tiled reader usually goes wrong: writing a tile's
    /// pixels at the wrong offset gives a raster with the right histogram and the wrong
    /// geometry, so totals look right while locations are not.
    fn blit(
        &self,
        chunk: &DecodingResult,
        tile_col: u32,
        tile_row: u32,
        window: PixelWindow,
        out: &mut [f64],
        out_width: usize,
    ) {
        let tile_x = tile_col * self.tile_width;
        let tile_y = tile_row * self.tile_height;

        // Edge tiles are padded to full tile size in the file, so the decoded buffer is
        // always tile_width wide even where the image is narrower.
        let left = window.x.max(tile_x);
        let top = window.y.max(tile_y);
        let right = (window.x + window.width).min(tile_x + self.tile_width);
        let bottom = (window.y + window.height).min(tile_y + self.tile_height);

        for y in top..bottom {
            for x in left..right {
                let source = ((y - tile_y) * self.tile_width + (x - tile_x)) as usize;
                let target = (y - window.y) as usize * out_width + (x - window.x) as usize;
                if let Some(value) = sample(chunk, source) {
                    out[target] = value;
                }
            }
        }
    }
}

/// One sample from a decoded chunk, widened to `f64`.
fn sample(chunk: &DecodingResult, index: usize) -> Option<f64> {
    match chunk {
        DecodingResult::U8(values) => values.get(index).map(|&v| f64::from(v)),
        DecodingResult::U16(values) => values.get(index).map(|&v| f64::from(v)),
        DecodingResult::U32(values) => values.get(index).map(|&v| f64::from(v)),
        DecodingResult::U64(values) => values.get(index).map(|&v| v as f64),
        DecodingResult::I8(values) => values.get(index).map(|&v| f64::from(v)),
        DecodingResult::I16(values) => values.get(index).map(|&v| f64::from(v)),
        DecodingResult::I32(values) => values.get(index).map(|&v| f64::from(v)),
        DecodingResult::I64(values) => values.get(index).map(|&v| v as f64),
        DecodingResult::F32(values) => values.get(index).map(|&v| f64::from(v)),
        DecodingResult::F64(values) => values.get(index).copied(),
        // Rejected earlier with a clear error rather than read as zero here.
        DecodingResult::F16(_) => None,
    }
}

/// A prefix of a file, which reports when something past it was wanted.
///
/// Used to discover how far a header reaches: rather than reimplementing the IFD layout
/// to predict the size, the decoder is allowed to ask, and the first unmet request is
/// what triggers a wider fetch.
struct PartialFile {
    bytes: Vec<u8>,
    size: u64,
    position: u64,
    ran_short: std::rc::Rc<std::cell::Cell<bool>>,
}

impl PartialFile {
    fn new(bytes: Vec<u8>, size: u64) -> Self {
        Self {
            bytes,
            size,
            position: 0,
            ran_short: std::rc::Rc::new(std::cell::Cell::new(false)),
        }
    }
}

impl Read for PartialFile {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let position = usize::try_from(self.position).unwrap_or(usize::MAX);
        if position >= self.bytes.len() {
            self.ran_short.set(true);
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "past the fetched prefix",
            ));
        }
        let taken = out.len().min(self.bytes.len() - position);
        out[..taken].copy_from_slice(&self.bytes[position..position + taken]);
        self.position += taken as u64;
        Ok(taken)
    }
}

impl Seek for PartialFile {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.position = resolve(to, self.position, self.size)?;
        Ok(self.position)
    }
}

/// Fetched byte ranges, presented to the TIFF decoder as a seekable file.
///
/// Reads outside what was fetched fail rather than returning zeros, so a missing tile
/// surfaces as an error instead of an empty patch of raster.
struct SeekableSparse {
    bytes: SparseBytes,
    position: u64,
}

impl SeekableSparse {
    fn new(bytes: SparseBytes) -> Self {
        Self { bytes, position: 0 }
    }
}

impl Read for SeekableSparse {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let available = self.bytes.contiguous_from(self.position).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("bytes at {} were not fetched", self.position),
            )
        })?;
        let taken = out.len().min(available.len());
        out[..taken].copy_from_slice(&available[..taken]);
        self.position += taken as u64;
        Ok(taken)
    }
}

impl Seek for SeekableSparse {
    fn seek(&mut self, to: SeekFrom) -> std::io::Result<u64> {
        self.position = resolve(to, self.position, self.bytes.file_size())?;
        Ok(self.position)
    }
}

/// Resolve a seek against a current position and size.
fn resolve(to: SeekFrom, position: u64, size: u64) -> std::io::Result<u64> {
    let target = match to {
        SeekFrom::Start(offset) => i128::from(offset),
        SeekFrom::End(offset) => i128::from(size) + i128::from(offset),
        SeekFrom::Current(offset) => i128::from(position) + i128::from(offset),
    };
    u64::try_from(target).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "seek to a negative position",
        )
    })
}
