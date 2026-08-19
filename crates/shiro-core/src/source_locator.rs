//! Parser-neutral source evidence coordinates.
//!
//! Canonical text spans remain the text coordinate system. Source locators are
//! validated provenance projections into physical source pages (ADR-035).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLocatorError {
    message: String,
}

impl SourceLocatorError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for SourceLocatorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SourceLocatorError {}

/// Coordinate origin declared by the parser that produced a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateOrigin {
    TopLeft,
    BottomLeft,
}

/// Axis-aligned parser-native page region. Values are not normalized.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "RawSourceRegion")]
pub struct SourceRegion {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct RawSourceRegion {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

impl SourceRegion {
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Result<Self, SourceLocatorError> {
        if [x0, y0, x1, y1]
            .iter()
            .any(|coordinate| !coordinate.is_finite())
        {
            return Err(SourceLocatorError::new(
                "source region coordinates must be finite",
            ));
        }
        Ok(Self { x0, y0, x1, y1 })
    }

    pub fn x0(self) -> f64 {
        self.x0
    }

    pub fn y0(self) -> f64 {
        self.y0
    }

    pub fn x1(self) -> f64 {
        self.x1
    }

    pub fn y1(self) -> f64 {
        self.y1
    }
}

impl TryFrom<RawSourceRegion> for SourceRegion {
    type Error = SourceLocatorError;

    fn try_from(raw: RawSourceRegion) -> Result<Self, Self::Error> {
        Self::new(raw.x0, raw.y0, raw.x1, raw.y1)
    }
}

/// Parser-native page dimensions in the same coordinate space as a region.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "RawPageDimensions")]
pub struct PageDimensions {
    width: f64,
    height: f64,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct RawPageDimensions {
    width: f64,
    height: f64,
}

impl PageDimensions {
    pub fn new(width: f64, height: f64) -> Result<Self, SourceLocatorError> {
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(SourceLocatorError::new(
                "page dimensions must be finite and positive",
            ));
        }
        Ok(Self { width, height })
    }

    pub fn width(self) -> f64 {
        self.width
    }

    pub fn height(self) -> f64 {
        self.height
    }
}

impl TryFrom<RawPageDimensions> for PageDimensions {
    type Error = SourceLocatorError;

    fn try_from(raw: RawPageDimensions) -> Result<Self, Self::Error> {
        Self::new(raw.width, raw.height)
    }
}

/// Validated physical source position for a canonical block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "RawSourceLocator")]
pub struct SourceLocator {
    page_number: u32,
    region: Option<SourceRegion>,
    coordinate_origin: Option<CoordinateOrigin>,
    page_dimensions: Option<PageDimensions>,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct RawSourceLocator {
    page_number: u32,
    #[serde(default)]
    region: Option<SourceRegion>,
    #[serde(default)]
    coordinate_origin: Option<CoordinateOrigin>,
    #[serde(default)]
    page_dimensions: Option<PageDimensions>,
}

impl SourceLocator {
    pub fn new(
        page_number: u32,
        region: Option<SourceRegion>,
        coordinate_origin: Option<CoordinateOrigin>,
        page_dimensions: Option<PageDimensions>,
    ) -> Result<Self, SourceLocatorError> {
        if page_number == 0 {
            return Err(SourceLocatorError::new(
                "source locator page number must be one-based and nonzero",
            ));
        }
        if coordinate_origin.is_some() && region.is_none() {
            return Err(SourceLocatorError::new(
                "coordinate origin requires a source region",
            ));
        }
        Ok(Self {
            page_number,
            region,
            coordinate_origin,
            page_dimensions,
        })
    }

    pub fn page_number(&self) -> u32 {
        self.page_number
    }

    pub fn region(&self) -> Option<SourceRegion> {
        self.region
    }

    pub fn coordinate_origin(&self) -> Option<CoordinateOrigin> {
        self.coordinate_origin
    }

    pub fn page_dimensions(&self) -> Option<PageDimensions> {
        self.page_dimensions
    }
}

impl TryFrom<RawSourceLocator> for SourceLocator {
    type Error = SourceLocatorError;

    fn try_from(raw: RawSourceLocator) -> Result<Self, Self::Error> {
        Self::new(
            raw.page_number,
            raw.region,
            raw.coordinate_origin,
            raw.page_dimensions,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_page_geometry() {
        assert!(SourceLocator::new(0, None, None, None).is_err());
        assert!(SourceRegion::new(f64::NAN, 0.0, 1.0, 1.0).is_err());
        assert!(PageDimensions::new(0.0, 100.0).is_err());
        assert!(PageDimensions::new(100.0, f64::INFINITY).is_err());
    }

    #[test]
    fn serde_revalidates_source_locator() {
        let invalid =
            r#"{"page_number":0,"region":null,"coordinate_origin":null,"page_dimensions":null}"#;
        assert!(serde_json::from_str::<SourceLocator>(invalid).is_err());

        let locator = SourceLocator::new(
            2,
            Some(SourceRegion::new(1.0, 2.0, 3.0, 4.0).unwrap()),
            Some(CoordinateOrigin::TopLeft),
            Some(PageDimensions::new(612.0, 792.0).unwrap()),
        )
        .unwrap();
        let restored: SourceLocator =
            serde_json::from_str(&serde_json::to_string(&locator).unwrap()).unwrap();
        assert_eq!(restored, locator);
    }
}
