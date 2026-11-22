use serde::{Deserialize, Serialize};

use caekit_types::Error;
use caekit_types::error::{nonempty, positive};

/// Identity of the geometry on which the mesh and boundary assignments are based.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeometryRevision {
    /// Identifier of the geometry the model was built on; must contain a non-whitespace character.
    pub id: String,
    /// Revision label of that geometry; must contain a non-whitespace character. The run report
    /// repeats `id` and `revision` under `geometry`.
    pub revision: String,
}

impl GeometryRevision {
    /// Checks that `id` and `revision` are not blank.
    ///
    /// `path` is the model path of this object, for example `physics.geometry`; it prefixes the
    /// field name in the returned [`Error::InvalidModel`].
    pub fn validate(&self, path: &str) -> Result<(), Error> {
        nonempty(&format!("{path}.id"), &self.id)?;
        nonempty(&format!("{path}.revision"), &self.revision)
    }
}

/// Largest accepted distance of the Euclidean norm of an [`AxisFrame`] direction from one.
///
/// Dimensionless. Reason: rounding to binary64 moves the norm of a three-component vector by a few
/// times 1.1e-16, so the bound is four orders above rounding. A direction that a person writes to
/// 12 decimals has components off by at most 5e-13, which moves the norm by at most
/// 5e-13 * sqrt(3) = 8.7e-13 (the first-order change is the sum of the components times their
/// errors, largest for the diagonal direction), so such a direction is accepted. The frame is
/// not renormalized, so the accepted deviation scales every position along the axis by at most
/// one part in 1e12.
const UNIT_DIRECTION_TOLERANCE: f64 = 1.0e-12;

/// An oriented one-dimensional coordinate, embedded in the global Cartesian frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxisFrame {
    /// Global Cartesian position, in metres, of the point at local coordinate zero; every component
    /// must be finite.
    pub origin_m: [f64; 3],
    /// Direction of increasing local coordinate as global Cartesian components (dimensionless). Its
    /// Euclidean norm must equal one to within 1e-12.
    pub direction: [f64; 3],
}

impl AxisFrame {
    /// Checks that the origin is finite and the direction is a unit vector.
    ///
    /// Returns [`Error::InvalidModel`] at `path` when an origin component is NaN or infinite, or
    /// when the Euclidean norm of `direction` is not finite or differs from one by more than 1e-12.
    pub fn validate(&self, path: &str) -> Result<(), Error> {
        if self.origin_m.iter().any(|x| !x.is_finite()) {
            return Err(Error::invalid(path, "frame origin must be finite"));
        }
        let norm = crate::length(self.direction);
        if !norm.is_finite() || (norm - 1.0).abs() > UNIT_DIRECTION_TOLERANCE {
            return Err(Error::invalid(
                path,
                "frame direction must be a unit vector",
            ));
        }
        Ok(())
    }

    /// Maps a local coordinate to the global frame.
    ///
    /// Returns `origin_m + distance_m * direction` in metres, where `distance_m` is the signed
    /// distance along the axis from the origin. The frame is used as given; nothing is validated
    /// here.
    pub fn position(&self, distance_m: f64) -> [f64; 3] {
        std::array::from_fn(|i| self.origin_m[i] + distance_m * self.direction[i])
    }
}

/// Contiguous segments between ordered vertices. The two names identify the exterior endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineMesh {
    /// Vertex coordinates along the local axis in metres, in strictly increasing order.
    ///
    /// At least two are required. The first must be exactly 0 and the last exactly the geometric
    /// extent given to [`LineMesh::validate`]. Cell `i` (zero-based) spans vertices `i` and `i +
    /// 1`.
    pub vertices_m: Vec<f64>,
    /// Name of the boundary region at the first vertex (local coordinate 0). Must not be blank and
    /// must differ from `end_region`.
    pub start_region: String,
    /// Name of the boundary region at the last vertex (local coordinate equal to the extent). Must
    /// not be blank and must differ from `start_region`.
    pub end_region: String,
}

/// Cell count, cell size range and, for tetrahedra, worst shape quality of one validated mesh.
///
/// Model checks and run reports list one entry per mesh under `meshes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshMetrics {
    /// Model path of the mesh as given to its validation, for example `physics.mesh`.
    pub name: String,
    /// Number of cells: intervals of a line mesh or tetrahedra of a tetrahedral mesh.
    pub cell_count: usize,
    /// Smallest cell size in metres: the shortest interval of a line mesh, or the shortest edge of
    /// any tetrahedron.
    pub smallest_cell_m: f64,
    /// Zero-based index, in input order, of the first cell that attains `smallest_cell_m`.
    pub smallest_cell_index: usize,
    /// Largest cell size in metres: the longest interval of a line mesh, or the longest edge of any
    /// tetrahedron.
    pub largest_cell_m: f64,
    /// Worst cell shape quality. Present for tetrahedral meshes; `None` for line meshes, in which
    /// case the key is omitted from JSON.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<MeshQuality>,
}

/// Lowest mean-ratio shape quality among the tetrahedra of a mesh, and the cell that has it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshQuality {
    /// Smallest mean ratio over all cells (dimensionless).
    ///
    /// For a cell of volume `V` the mean ratio is `12 (3 V)^(2/3)` divided by the sum of its six
    /// squared edge lengths, capped at 1. It is 1 for a regular tetrahedron and tends to 0 as the
    /// cell degenerates.
    pub minimum_mean_ratio: f64,
    /// Zero-based index, in input order, of the first cell that attains `minimum_mean_ratio`.
    pub cell_index: usize,
}

impl LineMesh {
    /// Checks the vertices and endpoint names against a geometric extent and measures the cells.
    ///
    /// `length` is the extent in metres that the mesh must span. `path` is the model path of the
    /// mesh; it prefixes error paths and becomes [`MeshMetrics::name`]. Returns
    /// [`Error::InvalidModel`] when there are fewer than two vertices, the first is not exactly 0,
    /// the last is not exactly `length`, a region name is blank, the two region names are equal, or
    /// a cell length is not positive and finite. On success the metrics hold the cell count and the
    /// smallest and largest cell lengths; `quality` is `None`.
    pub fn validate(&self, path: &str, length: f64) -> Result<MeshMetrics, Error> {
        let vertices = &self.vertices_m;
        if vertices.len() < 2 || vertices.first() != Some(&0.0) || vertices.last() != Some(&length)
        {
            return Err(Error::invalid(
                format!("{path}.vertices_m"),
                "requires at least two vertices spanning zero to the geometric extent",
            ));
        }
        nonempty(&format!("{path}.start_region"), &self.start_region)?;
        nonempty(&format!("{path}.end_region"), &self.end_region)?;
        if self.start_region == self.end_region {
            return Err(Error::invalid(path, "endpoint region names must differ"));
        }

        let mut metrics = MeshMetrics {
            name: path.to_owned(),
            cell_count: vertices.len() - 1,
            smallest_cell_m: f64::INFINITY,
            smallest_cell_index: 0,
            largest_cell_m: 0.0,
            quality: None,
        };
        for (i, edge) in vertices.windows(2).enumerate() {
            let length = edge[1] - edge[0];
            positive(&format!("{path}.vertices_m[{}]", i + 1), length)?;
            if length < metrics.smallest_cell_m {
                metrics.smallest_cell_m = length;
                metrics.smallest_cell_index = i;
            }
            metrics.largest_cell_m = metrics.largest_cell_m.max(length);
        }
        Ok(metrics)
    }

    /// Returns the vertex index of the endpoint whose region is named `region`.
    ///
    /// Gives `Some(0)` for `start_region`, the index of the last vertex for `end_region`, and
    /// `None` for any other name. Meant for a validated mesh, where the vertex list is nonempty and
    /// the two names differ.
    pub fn endpoint(&self, region: &str) -> Option<usize> {
        if region == self.start_region {
            Some(0)
        } else if region == self.end_region {
            Some(self.vertices_m.len() - 1)
        } else {
            None
        }
    }
}
