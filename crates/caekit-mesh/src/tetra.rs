//! Conforming tetrahedral meshes and geometric operators in global coordinates.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{MeshMetrics, MeshQuality, length};
use caekit_types::Error;
use caekit_types::error::nonempty;

#[cfg(test)]
mod tests {
    use super::{FaceRegion, TetraMesh};

    #[test]
    fn portable_cube_root_matches_the_binary64_reference() {
        // Round the positive root of 2 x³ = 1 to binary64. This operand also
        // exercises the nontrivial path when the test runs under WASI.
        assert_eq!(libm::cbrt(0.5).to_bits(), 0x3fe9_65fe_a53d_6e3d);
    }

    #[test]
    fn mean_ratio_is_one_for_a_regular_tetrahedron_and_falls_under_flattening() {
        // Alternate corners of a cube: every edge is sqrt(8), the determinant of the edge vectors
        // is 16 (vertex order chosen for positive orientation), so 12 (3V)^(2/3) / sum of l^2 =
        // 12 * 8^(2/3) / (6 * 8) = 1 exactly (Liu and Joe 1994).
        let regular = [
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, -1.0],
            [1.0, -1.0, -1.0],
            [-1.0, -1.0, 1.0],
        ];
        let (_, _, _, quality) = super::element(regular).expect("the regular tetrahedron is valid");
        // Four ulp of 1 (2^-52 each): the edge vectors are scaled by the longest edge sqrt(8),
        // which is inexact, before the determinant, the correctly rounded cube root and the
        // squared edge lengths are formed, each adding at most about one ulp.
        let rounding = 4.0 * f64::EPSILON;
        assert!((quality - 1.0).abs() <= rounding, "{quality}");

        // Pressing the fourth vertex towards the opposite face lowers the mean ratio towards zero.
        // The face centroid is (1, 1, -1)/3 and the vertex sits 4/3 (-1, -1, 1) above it; keeping
        // the fraction `s` of that height scales the volume by `s`.
        let mut previous = quality;
        for s in [0.5, 0.1, 0.01] {
            let mut flattened = regular;
            let offset = s * 4.0 / 3.0;
            flattened[3] = [1.0 / 3.0 - offset, 1.0 / 3.0 - offset, -1.0 / 3.0 + offset];
            let (_, _, _, flat) = super::element(flattened).expect("still positively oriented");
            assert!(flat < previous, "{s}: {flat} not below {previous}");
            previous = flat;
        }
        // At s = 0.01, (3V)^(2/3) falls by 0.01^(2/3) = 0.046, and the sum of squared edges falls
        // from 48 to about 3 * 8 + 3 * 8 / 3 = 32, so the ratio is about 0.046 * 48 / 32 = 0.07.
        assert!(previous < 0.1, "{previous}");
    }

    #[test]
    fn tetrahedral_entity_ids_follow_vertex_indices_and_ignore_region_order() {
        let mut mesh = TetraMesh {
            vertices_m: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                [0.0, 0.0, -1.0],
            ],
            cells: vec![[0, 1, 2, 3], [0, 2, 1, 4]],
            boundary_regions: vec![
                FaceRegion {
                    name: "top".into(),
                    faces: vec![[1, 2, 3], [0, 2, 3], [0, 1, 3]],
                },
                FaceRegion {
                    name: "bottom".into(),
                    faces: vec![[1, 2, 4], [0, 1, 4], [0, 2, 4]],
                },
            ],
        };
        let entities = mesh.checked("mesh").unwrap().entities().unwrap();
        assert_eq!(
            entities.edges,
            [
                [0, 1],
                [0, 2],
                [0, 3],
                [0, 4],
                [1, 2],
                [1, 3],
                [1, 4],
                [2, 3],
                [2, 4]
            ]
        );
        assert_eq!(
            entities.faces,
            [
                [0, 1, 2],
                [0, 1, 3],
                [0, 1, 4],
                [0, 2, 3],
                [0, 2, 4],
                [1, 2, 3],
                [1, 2, 4]
            ]
        );
        assert_eq!(
            entities.cell_edges,
            [[0, 1, 2, 4, 5, 7], [1, 0, 3, 4, 8, 6]]
        );
        assert_eq!(entities.cell_faces, [[5, 3, 1, 0], [6, 2, 4, 0]]);
        mesh.boundary_regions.reverse();
        for region in &mut mesh.boundary_regions {
            region.faces.reverse();
            for face in &mut region.faces {
                face.reverse();
            }
        }
        assert_eq!(mesh.checked("mesh").unwrap().entities().unwrap(), entities);
    }
}

/// Tetrahedral volume mesh in global Cartesian coordinates with named exterior face regions.
///
/// The fields are the keys of a solid `mesh` object in the JSON model. All vertex and cell indices
/// are zero-based. [`TetraMesh::check`] enforces the constraints stated on the fields and requires
/// the cells to form one body connected through shared faces.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TetraMesh {
    /// Vertex positions as global Cartesian `[x, y, z]` in metres. At least four are required,
    /// every coordinate must be finite and every vertex must belong to at least one cell.
    pub vertices_m: Vec<[f64; 3]>,
    /// Positive orientation, with the three edge vectors measured from the first vertex.
    ///
    /// Each cell lists four distinct zero-based indices into `vertices_m`. At least one cell is
    /// required, and no two cells may have the same vertex set.
    pub cells: Vec<[usize; 4]>,
    /// Named sets of exterior triangles. Each exterior face belongs to one region.
    pub boundary_regions: Vec<FaceRegion>,
}

/// Named set of exterior triangles that boundary conditions refer to.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaceRegion {
    /// Region name used by boundary conditions; must not be blank and must be unique within the
    /// mesh.
    pub name: String,
    /// Triangles as three zero-based vertex indices in any order. The list must not be empty, and
    /// each triangle must be an exterior face of the mesh that no other entry of any region claims.
    pub faces: Vec<[usize; 3]>,
}

/// Returns the component-wise difference `a - b` of two 3-vectors.
pub fn subtract(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

/// Returns the right-handed cross product `a × b` of two 3-vectors.
pub fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Returns the scalar product `a · b` of two 3-vectors.
pub fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

/// Geometry of one checked tetrahedron as needed by linear finite-element operators.
#[derive(Debug)]
pub struct Tet {
    /// Cell volume in m³; positive and finite.
    pub volume: f64,
    /// Gradients of the four linear nodal shape functions in 1/m, as global Cartesian components.
    /// Entry `a` belongs to vertex `a` of the cell in the order of [`TetraMesh::cells`]. Each
    /// gradient is constant over the cell, and the first is the negated sum of the other three.
    pub gradients: [[f64; 3]; 4],
    /// Mean of the four vertex positions in metres, in global Cartesian coordinates.
    pub centroid: [f64; 3],
}

/// Exterior triangular face of a checked mesh with its area and outward normal.
#[derive(Debug)]
pub struct Triangle {
    /// Zero-based vertex indices of the face, sorted in ascending order.
    pub nodes: [usize; 3],
    /// Face area in m²; positive and finite.
    pub area: f64,
    /// Unit normal pointing out of the tetrahedron that owns the face, as global Cartesian
    /// components.
    pub outward: [f64; 3],
}

/// Triangular face shared by two tetrahedra of a checked mesh.
pub struct InteriorFace {
    /// Zero-based indices of the two cells that share the face; the first is the one listed earlier
    /// in [`TetraMesh::cells`].
    pub cells: [usize; 2],
    /// Face area in m²; positive and finite.
    pub area: f64,
    /// Unit normal from the first cell toward the second.
    pub outward: [f64; 3],
}

/// Cell ownership of the exterior and interior faces of a checked mesh.
///
/// Filled by [`TetraMesh::checked_with_face_adjacency`]. The map keys borrow the region names of
/// the mesh.
#[derive(Default)]
pub struct TetraFaceAdjacency<'a> {
    /// Exterior face owner indices in the checked topology's boundary face order.
    pub boundary_cells: BTreeMap<&'a str, Vec<usize>>,
    /// Every face shared by two cells, in ascending order of the face's sorted vertex-index triple.
    pub interior_faces: Vec<InteriorFace>,
}

/// Element geometry, boundary triangles and metrics derived from a checked tetrahedral mesh.
///
/// The map keys borrow the region names of the mesh.
pub struct TetraTopology<'a> {
    /// Volume, shape-function gradients and centroid of each cell, in the order of
    /// [`TetraMesh::cells`].
    pub elements: Vec<Tet>,
    /// Exterior triangles by region name. Each list follows the order of that region's
    /// [`FaceRegion::faces`].
    pub boundaries: BTreeMap<&'a str, Vec<Triangle>>,
    /// Cell count, shortest and longest edge with the first cell holding the shortest, and the
    /// lowest mean-ratio quality. Its `name` is the path given to the check.
    pub metrics: MeshMetrics,
}

/// A checked topology bound to the mesh whose node and cell ordering it follows.
pub struct CheckedTetraMesh<'a> {
    mesh: &'a TetraMesh,
    topology: TetraTopology<'a>,
}

/// Zero-based edge and face numbering of a checked tetrahedral mesh.
///
/// IDs depend only on vertex indices and are independent of boundary region order.
#[derive(Debug, PartialEq, Eq)]
pub struct TetraEntities {
    /// Global edge table in lexicographic order, with each vertex pair in ascending order.
    pub edges: Vec<[usize; 2]>,
    /// Global face table in lexicographic order, with each vertex triple in ascending order.
    /// Includes both interior and exterior faces.
    pub faces: Vec<[usize; 3]>,
    /// Edge IDs by cell in the mesh's cell order. Local pairs follow
    /// (0,1), (0,2), (0,3), (1,2), (1,3), (2,3).
    pub cell_edges: Vec<[usize; 6]>,
    /// Face IDs by cell in the mesh's cell order. Local face `i` is opposite local vertex `i`.
    pub cell_faces: Vec<[usize; 4]>,
}

impl<'a> CheckedTetraMesh<'a> {
    /// Returns the mesh that was checked.
    pub fn mesh(&self) -> &'a TetraMesh {
        self.mesh
    }

    /// Returns the element geometry, boundary triangles and metrics derived by the check.
    pub fn topology(&self) -> &TetraTopology<'a> {
        &self.topology
    }

    /// Number the unique edges and faces on demand.
    ///
    /// Returns [`Error::InsufficientMemory`] if the entity tables cannot be sized or reserved.
    pub fn entities(&self) -> Result<TetraEntities, Error> {
        let (edges, cell_edges) = number_entities(&self.mesh.cells, |[a, b, c, d]| {
            [[a, b], [a, c], [a, d], [b, c], [b, d], [c, d]]
        })?;
        let (faces, cell_faces) = number_entities(&self.mesh.cells, |[a, b, c, d]| {
            [[b, c, d], [a, c, d], [a, b, d], [a, b, c]]
        })?;
        Ok(TetraEntities {
            edges,
            faces,
            cell_edges,
            cell_faces,
        })
    }

    /// Consumes the checked mesh and returns its [`MeshMetrics`], discarding the derived geometry.
    pub fn into_metrics(self) -> MeshMetrics {
        self.topology.metrics
    }
}

type EntityNumbering<const NODES: usize, const PER_CELL: usize> =
    (Vec<[usize; NODES]>, Vec<[usize; PER_CELL]>);

fn number_entities<const NODES: usize, const PER_CELL: usize>(
    cells: &[[usize; 4]],
    local: impl Fn([usize; 4]) -> [[usize; NODES]; PER_CELL],
) -> Result<EntityNumbering<NODES, PER_CELL>, Error> {
    let memory = || Error::InsufficientMemory {
        resource: "tetrahedral entity numbering".to_owned(),
    };
    let mut keys = Vec::new();
    let mut ids = Vec::new();
    let count = cells.len().checked_mul(PER_CELL).ok_or_else(memory)?;
    keys.try_reserve_exact(count).map_err(|_| memory())?;
    ids.try_reserve_exact(cells.len()).map_err(|_| memory())?;
    let sorted_keys = |cell| {
        local(cell).map(|mut nodes| {
            nodes.sort_unstable();
            nodes
        })
    };
    for &cell in cells {
        keys.extend(sorted_keys(cell));
    }
    keys.sort_unstable();
    keys.dedup();
    for &cell in cells {
        ids.push(sorted_keys(cell).map(|nodes| {
            keys.binary_search(&nodes)
                .expect("the sorted entity table contains every cell entity")
        }));
    }
    Ok((keys, ids))
}

#[derive(Debug)]
struct FaceUse {
    first: usize,
    normal_points_in: bool,
    other: Option<usize>,
    boundary: Triangle,
}

impl TetraMesh {
    /// Validates the mesh as [`TetraMesh::check`] does and binds the derived topology to the mesh.
    ///
    /// Error conditions and the meaning of `path` are those of `check`.
    pub fn checked<'a>(&'a self, path: &str) -> Result<CheckedTetraMesh<'a>, Error> {
        Ok(CheckedTetraMesh {
            mesh: self,
            topology: self.check(path)?,
        })
    }

    /// Validates the mesh and derives element geometry, boundary triangles and mesh metrics.
    ///
    /// `path` is the model path of the mesh; it prefixes error paths and becomes
    /// [`MeshMetrics::name`]. Returns [`Error::InvalidModel`] when:
    ///
    /// - there are fewer than four vertices or no cells, or a coordinate is not finite;
    /// - a cell has an out-of-range or repeated vertex index, has the same vertex set as an earlier
    ///   cell, or is inverted, degenerate or outside the finite binary64 range;
    /// - a face has a numerically degenerate area or orientation, is shared by more than two cells,
    ///   or has both of its cells on the same side of it;
    /// - a vertex belongs to no cell, or the cells are not all connected through shared faces;
    /// - a region name is blank or repeated, a region has no faces, or a listed face is unknown,
    ///   interior or already assigned;
    /// - an exterior face belongs to no region.
    ///
    /// The checks assume a conforming volume that does not intersect itself.
    pub fn check<'a>(&'a self, path: &str) -> Result<TetraTopology<'a>, Error> {
        self.check_topology(path, None)
    }

    /// Check the mesh and retain cell ownership and orientation for heat exchange on its faces.
    pub fn checked_with_face_adjacency<'a>(
        &'a self,
        path: &str,
    ) -> Result<(CheckedTetraMesh<'a>, TetraFaceAdjacency<'a>), Error> {
        let mut adjacency = TetraFaceAdjacency::default();
        let topology = self.check_topology(path, Some(&mut adjacency))?;
        Ok((
            CheckedTetraMesh {
                mesh: self,
                topology,
            },
            adjacency,
        ))
    }

    fn check_topology<'a>(
        &'a self,
        path: &str,
        mut adjacency: Option<&mut TetraFaceAdjacency<'a>>,
    ) -> Result<TetraTopology<'a>, Error> {
        let vertices = &self.vertices_m;
        if vertices.len() < 4 || self.cells.is_empty() {
            return Err(Error::invalid(
                path,
                "requires at least four vertices and one tetrahedron",
            ));
        }
        for (i, point) in vertices.iter().enumerate() {
            if !point.iter().all(|x| x.is_finite()) {
                return Err(Error::invalid(
                    format!("{path}.vertices_m[{i}]"),
                    "coordinates must be finite",
                ));
            }
        }

        let mut elements = Vec::with_capacity(self.cells.len());
        let mut faces: BTreeMap<[usize; 3], FaceUse> = BTreeMap::new();
        let mut cell_ids = BTreeSet::new();
        let mut used = vec![false; vertices.len()];
        let mut parent: Vec<_> = (0..self.cells.len()).collect();
        let mut metrics = MeshMetrics {
            name: path.to_owned(),
            cell_count: self.cells.len(),
            smallest_cell_m: f64::INFINITY,
            smallest_cell_index: 0,
            largest_cell_m: 0.0,
            quality: Some(MeshQuality {
                minimum_mean_ratio: f64::INFINITY,
                cell_index: 0,
            }),
        };

        for (i, &nodes) in self.cells.iter().enumerate() {
            let mut ids = nodes;
            ids.sort_unstable();
            if ids[3] >= vertices.len() || ids.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(cell_error(
                    path,
                    i,
                    "tetrahedron requires four distinct in-range vertices",
                ));
            }
            if !cell_ids.insert(ids) {
                return Err(cell_error(path, i, "duplicate tetrahedron"));
            }
            for node in nodes {
                used[node] = true;
            }
            let points = nodes.map(|node| vertices[node]);
            let (element, shortest, longest, quality) = element(points).ok_or_else(|| {
                cell_error(
                    path,
                    i,
                    "tetrahedron is inverted, degenerate, or outside the finite geometric range",
                )
            })?;
            elements.push(element);
            if shortest < metrics.smallest_cell_m {
                metrics.smallest_cell_m = shortest;
                metrics.smallest_cell_index = i;
            }
            metrics.largest_cell_m = metrics.largest_cell_m.max(longest);
            let worst = metrics
                .quality
                .as_mut()
                .expect("tetrahedral metrics start with a quality record");
            if quality < worst.minimum_mean_ratio {
                worst.minimum_mean_ratio = quality;
                worst.cell_index = i;
            }

            for (opposite, &opposite_point) in points.iter().enumerate() {
                let mut face = [0; 3];
                for (target, node) in face.iter_mut().zip(
                    nodes
                        .iter()
                        .enumerate()
                        .filter_map(|(j, &node)| (j != opposite).then_some(node)),
                ) {
                    *target = node;
                }
                face.sort_unstable();
                let [p, q, r] = face.map(|node| vertices[node]);
                let (area, normal, normal_points_in) =
                    face_geometry(p, q, r, opposite_point, longest).ok_or_else(|| {
                        cell_error(
                            path,
                            i,
                            "face area or orientation is numerically degenerate",
                        )
                    })?;

                match faces.entry(face) {
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(FaceUse {
                            first: i,
                            normal_points_in,
                            other: None,
                            boundary: Triangle {
                                nodes: face,
                                area,
                                outward: normal,
                            },
                        });
                    }
                    std::collections::btree_map::Entry::Occupied(mut entry) => {
                        let owner = entry.get_mut();
                        if owner.other.is_some() {
                            return Err(cell_error(
                                path,
                                i,
                                "a triangle is shared by more than two tetrahedra",
                            ));
                        }
                        if owner.normal_points_in == normal_points_in {
                            return Err(cell_error(
                                path,
                                i,
                                "adjacent tetrahedra lie on the same side of their shared face",
                            ));
                        }
                        owner.other = Some(i);
                        let a = root(&mut parent, owner.first);
                        let b = root(&mut parent, i);
                        parent[b] = a;
                    }
                }
            }
        }

        if let Some(unused) = used.iter().position(|assigned| !assigned) {
            return Err(Error::invalid(
                format!("{path}.vertices_m[{unused}]"),
                "vertex is unused",
            ));
        }
        let first = root(&mut parent, 0);
        if let Some(disconnected) = (1..parent.len()).find(|&i| root(&mut parent, i) != first) {
            return Err(cell_error(
                path,
                disconnected,
                "the solid mesh must be connected through faces",
            ));
        }

        let mut boundaries = BTreeMap::new();
        for (i, region) in self.boundary_regions.iter().enumerate() {
            let assigned = &region.faces;
            let region = &region.name;
            let region_path = format!("{path}.boundary_regions[{i}]");
            let name_path = format!("{region_path}.name");
            nonempty(&name_path, region)?;
            if boundaries.contains_key(region.as_str()) {
                return Err(Error::invalid(name_path, "duplicate region name"));
            }
            if assigned.is_empty() {
                return Err(Error::invalid(
                    format!("{region_path}.faces"),
                    "region has no faces",
                ));
            }
            let mut triangles = Vec::with_capacity(assigned.len());
            let mut owners = adjacency.as_mut().map(|faces| {
                faces
                    .boundary_cells
                    .entry(region.as_str())
                    .or_insert_with(|| Vec::with_capacity(assigned.len()))
            });
            for (i, &face) in assigned.iter().enumerate() {
                let mut key = face;
                key.sort_unstable();
                let Some(owner) = faces.remove(&key).filter(|owner| owner.other.is_none()) else {
                    return Err(Error::invalid(
                        format!("{region_path}.faces[{i}]"),
                        "region references an unknown, interior, or already assigned face",
                    ));
                };
                if let Some(cells) = &mut owners {
                    cells.push(owner.first);
                }
                triangles.push(owner.boundary);
            }
            boundaries.insert(region.as_str(), triangles);
        }

        if let Some((nodes, _)) = faces.iter().find(|(_, owner)| owner.other.is_none()) {
            return Err(Error::invalid(
                format!("{path}.boundary_regions"),
                format!("exterior face {nodes:?} has no region assignment"),
            ));
        }
        if let Some(adjacency) = adjacency {
            adjacency.interior_faces = faces
                .into_values()
                .filter_map(|face| {
                    face.other.map(|other| InteriorFace {
                        cells: [face.first, other],
                        area: face.boundary.area,
                        outward: face.boundary.outward,
                    })
                })
                .collect();
        }
        Ok(TetraTopology {
            elements,
            boundaries,
            metrics,
        })
    }
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

fn cell_error(path: &str, cell: usize, message: &str) -> Error {
    Error::invalid(format!("{path}.cells[{cell}]"), message)
}

/// Smallest accepted triple product of the three edges at vertex 0, after each edge is divided by
/// the longest edge of the tetrahedron.
///
/// Dimensionless (it is six times the volume over the cube of the longest edge). Reason: the
/// scaled components have magnitude at most one, so each component of the cross product is off by
/// at most 2 epsilon (two products and a subtraction of values up to 2), and the dot product adds
/// about 9 epsilon from those errors and the products plus 6 epsilon from two additions of
/// partial sums up to 6: at most 15 epsilon in all. 64 epsilon is four times that bound, so a
/// determinant at or below it cannot be told from zero and the cell is rejected as degenerate.
const MIN_SCALED_DETERMINANT: f64 = 64.0 * f64::EPSILON;

fn element(p: [[f64; 3]; 4]) -> Option<(Tet, f64, f64, f64)> {
    let edges = [
        subtract(p[1], p[0]),
        subtract(p[2], p[0]),
        subtract(p[3], p[0]),
        subtract(p[2], p[1]),
        subtract(p[3], p[1]),
        subtract(p[3], p[2]),
    ];
    let lengths = edges.map(length);
    let shortest = lengths.into_iter().fold(f64::INFINITY, f64::min);
    let longest = lengths.into_iter().fold(0.0, f64::max);
    if shortest <= 0.0 || !longest.is_finite() {
        return None;
    }

    let [a, b, c, ..] = edges.map(|v| v.map(|x| x / longest));
    let determinant = dot(a, cross(b, c));
    if determinant <= MIN_SCALED_DETERMINANT || !determinant.is_finite() {
        return None;
    }
    let volume = ((determinant / 6.0) * longest * longest) * longest;
    if !volume.is_finite() || volume <= 0.0 {
        return None;
    }
    let [g1, g2, g3] =
        [cross(b, c), cross(c, a), cross(a, b)].map(|v| v.map(|x| (x / determinant) / longest));
    let g0 = std::array::from_fn(|i| -(g1[i] + g2[i] + g3[i]));
    let gradients = [g0, g1, g2, g3];
    if !gradients.iter().flatten().all(|x| x.is_finite()) {
        return None;
    }
    let squared_edges: f64 = lengths
        .into_iter()
        .map(|length| {
            let relative = length / longest;
            relative * relative
        })
        .sum();
    let volume_scale = libm::cbrt(determinant / 2.0);
    let quality = (12.0 * (volume_scale * volume_scale) / squared_edges).min(1.0);
    let centroid = std::array::from_fn(|i| p.iter().map(|point| point[i] * 0.25).sum());
    Some((
        Tet {
            volume,
            gradients,
            centroid,
        },
        shortest,
        longest,
        quality,
    ))
}

fn face_geometry(
    p: [f64; 3],
    q: [f64; 3],
    r: [f64; 3],
    opposite: [f64; 3],
    scale: f64,
) -> Option<(f64, [f64; 3], bool)> {
    let local = |a| subtract(a, p).map(|x| x / scale);
    let normal = cross(local(q), local(r));
    let magnitude = length(normal);
    let area = magnitude * 0.5 * scale * scale;
    let direction = dot(normal, local(opposite));
    if !area.is_finite() || area <= 0.0 || direction == 0.0 {
        return None;
    }
    let normal_points_in = direction > 0.0;
    let sign = if normal_points_in { -1.0 } else { 1.0 };
    let outward = normal.map(|x| sign * x / magnitude);
    Some((area, outward, normal_points_in))
}
