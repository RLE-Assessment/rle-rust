//! Validating `geo` metadata against the published `GeoParquet` schemas.
//!
//! The schemas are vendored — see `tools/vendor_geoparquet_schemas.py` — rather than
//! fetched, because validation has to work in CI without network and in a browser,
//! where fetching a schema mid-parse is not an option.
//!
//! # What this does and does not tell you
//!
//! It says the `geo` metadata is well-formed per the specification. It says nothing
//! about whether the metadata describes the file it sits in: a schema never sees the
//! parquet around it. Colombia's national ecosystems map validates cleanly here and
//! still broke this reader four different ways.
//!
//! [`super::GeoParquet::check_structure`] covers the other half, and neither
//! substitutes for the other.

use serde_json::Value;

use crate::geoparquet::{Finding, GeoMetadata, Severity};

/// The URI the release schema is registered under while compiling.
///
/// A `urn:` rather than its real URL, so nothing can be tempted to fetch it.
const SCHEMA_URL: &str = "urn:geoparquet:release";

/// One vendored schema: the version it validates, and its text.
struct Vendored {
    version: &'static str,
    source: &'static str,
}

/// The published release schemas, embedded at build time.
///
/// Each pins its own `version` with `const`, so the right one has to be chosen by what
/// a file declares — there is no "latest" that accepts everything.
const RELEASES: &[Vendored] = &[
    Vendored {
        version: "0.2.0",
        source: include_str!("../schemas/v0.2.0.json"),
    },
    Vendored {
        version: "0.3.0",
        source: include_str!("../schemas/v0.3.0.json"),
    },
    Vendored {
        version: "0.4.0",
        source: include_str!("../schemas/v0.4.0.json"),
    },
    Vendored {
        version: "1.0.0-beta.1",
        source: include_str!("../schemas/v1.0.0-beta.1.json"),
    },
    Vendored {
        version: "1.0.0-rc.1",
        source: include_str!("../schemas/v1.0.0-rc.1.json"),
    },
    Vendored {
        version: "1.0.0",
        source: include_str!("../schemas/v1.0.0.json"),
    },
    Vendored {
        version: "1.1.0",
        source: include_str!("../schemas/v1.1.0.json"),
    },
    Vendored {
        version: "2.0.0-rc.1",
        source: include_str!("../schemas/v2.0.0-rc.1.json"),
    },
];

/// Externally referenced schemas, registered under the URLs the releases cite.
///
/// The `crs` field `$ref`s PROJJSON, and different releases cite different versions of
/// it. Registering them by URL is what lets the reference resolve without a network
/// request — and resolving it matters: unresolved, a validator either fails outright or
/// skips the subschema, and skipping means every CRS appears valid.
const REFERENCED: &[(&str, &str)] = &[
    (
        "https://proj.org/schemas/v0.4/projjson.schema.json",
        include_str!("../schemas/proj.org_schemas_v0.4_projjson.schema.json"),
    ),
    (
        "https://proj.org/schemas/v0.5/projjson.schema.json",
        include_str!("../schemas/proj.org_schemas_v0.5_projjson.schema.json"),
    ),
    (
        "https://proj.org/schemas/v0.7/projjson.schema.json",
        include_str!("../schemas/proj.org_schemas_v0.7_projjson.schema.json"),
    ),
];

/// Versions for which a schema is vendored.
#[must_use]
pub fn published_schema_versions() -> Vec<&'static str> {
    RELEASES.iter().map(|entry| entry.version).collect()
}

/// The vendored schema text for a declared version, if there is one.
#[must_use]
pub fn schema_for_version(version: &str) -> Option<&'static str> {
    RELEASES
        .iter()
        .find(|entry| entry.version == version)
        .map(|entry| entry.source)
}

/// Validate `geo` metadata against the schema published for its declared version.
pub(crate) fn validate(geo: &GeoMetadata, raw: &str) -> Vec<Finding> {
    let Some(source) = schema_for_version(&geo.version) else {
        return vec![Finding {
            severity: Severity::Warning,
            message: format!(
                "this file declares GeoParquet version `{}`, which is not one of the \
                 published releases ({}), so its metadata cannot be validated",
                geo.version,
                published_schema_versions().join(", ")
            ),
        }];
    };

    let instance: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(error) => {
            return vec![Finding {
                severity: Severity::Error,
                message: format!("the `geo` metadata is not valid JSON: {error}"),
            }]
        }
    };

    let mut compiler = boon::Compiler::new();
    for (url, text) in REFERENCED {
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        if compiler.add_resource(url, value).is_err() {
            return vec![Finding {
                severity: Severity::Warning,
                message: format!("the vendored copy of {url} could not be registered"),
            }];
        }
    }

    let Ok(schema) = serde_json::from_str::<Value>(source) else {
        return vec![Finding {
            severity: Severity::Warning,
            message: "the vendored release schema is not valid JSON".to_owned(),
        }];
    };
    if compiler.add_resource(SCHEMA_URL, schema).is_err() {
        return vec![Finding {
            severity: Severity::Warning,
            message: "the vendored release schema could not be registered".to_owned(),
        }];
    }

    let mut compiled_set = boon::Schemas::new();
    let release = match compiler.compile(SCHEMA_URL, &mut compiled_set) {
        Ok(schema) => schema,
        Err(error) => {
            return vec![Finding {
                severity: Severity::Warning,
                message: format!("the published schema could not be compiled: {error}"),
            }]
        }
    };

    match compiled_set.validate(&instance, release) {
        Ok(()) => Vec::new(),
        Err(error) => {
            // The verbose form names the failing keyword and location, which is what
            // makes the finding actionable rather than merely discouraging.
            vec![Finding {
                severity: Severity::Error,
                message: format!(
                    "the `geo` metadata does not satisfy the published GeoParquet {} \
                     schema: {}",
                    geo.version,
                    error.to_string().replace('\n', "; ")
                ),
            }]
        }
    }
}
